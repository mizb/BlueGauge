use crate::bluetooth::{BluetoothInfo, find_bluetooth_devices, get_bluetooth_info};
use crate::config::*;
use crate::language::{Language, Localization};
use crate::notify::notify;
use crate::startup::{get_startup_status, set_startup};

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use image;
use tao::{
    event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy},
    platform::run_return::EventLoopExtRunReturn,
};
use tray_icon::{
    TrayIconBuilder,
    menu::{
        AboutMetadata, CheckMenuItem, IsMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem,
        Submenu,
    },
};

#[derive(Debug)]
enum TrayEvent {
    ForwardUpdate,
    SetTooltip,
    SetTrayInfo,
}

const ICON_DATA: &[u8] = include_bytes!("../resources/logo.ico");

pub async fn show_systray() -> Result<()> {
    loop_systray().await
}

async fn loop_systray() -> Result<()> {
    let (ini, ini_path) = ini()?;

    let (tooltip, menu_devices, blue_info) =
        get_bluetooth_tray_info(Arc::new(Mutex::new(ini.clone()))).await?;

    let mut low_battery_devices = HashMap::<String, bool>::new(); // HashMap<bluetooth_id: String, notified: bool>

    let messages = blue_info
        .iter()
        .filter_map(|device| {
            ini.notify_low_battery
                .filter(|&low_battery| device.battery < low_battery && device.status)
                .map(|_| {
                    low_battery_devices.insert(device.name.clone(), true);
                    format!("{}: {}% battery remaining", device.name, device.battery)
                })
        })
        .collect::<Vec<String>>()
        .join("\n");

    if !messages.is_empty() && !low_battery_devices.is_empty() {
        let title = format!(
            "Bluetooth Battery Below {}%",
            ini.notify_low_battery.unwrap_or(15)
        );
        let text = messages.trim();
        let mute = ini.notify_mute;
        notify(&title, text, mute)?;
    }

    let tray_menu = create_tray_menu(&menu_devices, &ini)?;
    let tray_icon = TrayIconBuilder::new()
        .with_menu_on_left_click(true)
        .with_icon(load_icon(ICON_DATA).map_err(|e| anyhow!("Failed to load icon - {e}"))?)
        .with_tooltip(tooltip.join("\n"))
        .with_menu(Box::new(tray_menu))
        .build()
        .context("Failed to build tray")?;
    let menu_channel = MenuEvent::receiver();

    let config = Arc::new(Mutex::new(ini));
    let tooltip = Arc::new(Mutex::new(tooltip));
    let menu_devices = Arc::new(Mutex::new(menu_devices));
    let blue_info = Arc::new(Mutex::new(blue_info));
    let update_menu_event = Arc::new(Mutex::new(false));
    let low_battery_devices = Arc::new(Mutex::new(low_battery_devices)); // HashMap<bluetooth_id: String, notified: bool>
    let updated_in_advance = Arc::new(Mutex::new(false));

    let mut event_loop = EventLoopBuilder::<TrayEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();
    let proxy_menu = event_loop.create_proxy();

    tokio::task::spawn({
        let config = Arc::clone(&config);
        let tooltip = Arc::clone(&tooltip);
        let menu_devices = Arc::clone(&menu_devices);
        let blue_info = Arc::clone(&blue_info);
        let update_menu_event = Arc::clone(&update_menu_event);
        let low_battery_devices = Arc::clone(&low_battery_devices);
        let updated_in_advance = Arc::clone(&updated_in_advance);

        async move {
            loop {
                let update_interval = config.lock().map_or(30, |c| c.update_interval);

                std::thread::sleep(std::time::Duration::from_secs(update_interval));

                if let Ok(mut updated_in_advance) = updated_in_advance.try_lock() {
                    if std::mem::replace(&mut *updated_in_advance, false) {
                        continue;
                    }
                } else {
                    continue;
                }

                if let Err(e) = update_tray(
                    Arc::clone(&config),
                    Arc::clone(&tooltip),
                    Arc::clone(&menu_devices),
                    Arc::clone(&blue_info),
                    Arc::clone(&update_menu_event),
                    Arc::clone(&low_battery_devices),
                    &proxy,
                )
                .await
                {
                    eprintln!("Failed to update tray: {e}");
                }
            }
        }
    });

    let config = Arc::clone(&config);
    let tooltip = Arc::clone(&tooltip);
    let menu_devices = Arc::clone(&menu_devices);
    let update_menu_event = Arc::clone(&update_menu_event);
    let updated_in_advance = Arc::clone(&updated_in_advance);

    let _return_code = event_loop.run_return(|event, _, control_flow| {
        *control_flow =
            ControlFlow::WaitUntil(std::time::Instant::now() + Duration::from_millis(100));

        if let tao::event::Event::UserEvent(tray_event) = event {
            match tray_event {
                TrayEvent::SetTooltip => {
                    if let Ok(t) = tooltip.lock() {
                        tray_icon
                            .set_tooltip(Some(t.join("\n")))
                            .expect("Failed to update tray tooltip");
                    } else {
                        println!("Failed to acquire 'tooltip' lock")
                    }
                }
                TrayEvent::SetTrayInfo => {
                    if let Ok(t) = tooltip.lock() {
                        tray_icon
                            .set_tooltip(Some(t.join("\n")))
                            .expect("Failed to update tray tooltip");
                    } else {
                        println!("Failed to acquire 'tooltip' lock")
                    }

                    match (menu_devices.lock(), config.lock()) {
                        (Ok(menu_devices), Ok(config)) => {
                            if let Ok(tray_menu) = create_tray_menu(&menu_devices, &config) {
                                tray_icon.set_menu(Some(Box::new(tray_menu)));
                            } else {
                                println!("Failed to update(set) tray menu")
                            }
                        }
                        (_, _) => println!("Failed to acquire 'menu_devices' or 'config' lock"),
                    }
                }
                TrayEvent::ForwardUpdate => {
                    let config = Arc::clone(&config);
                    let tooltip = Arc::clone(&tooltip);
                    let menu_devices = Arc::clone(&menu_devices);
                    let blue_info_clone = Arc::clone(&blue_info);
                    let update_menu_event = Arc::clone(&update_menu_event);
                    let low_battery_devices = Arc::clone(&low_battery_devices);
                    let updated_in_advance = Arc::clone(&updated_in_advance);
                    let proxy_menu = proxy_menu.clone();
                    if let Ok(mut updated_in_advance) = updated_in_advance.lock() {
                        *updated_in_advance = true;
                        tokio::spawn(async move {
                            if let Err(e) = update_tray(
                                Arc::clone(&config),
                                Arc::clone(&tooltip),
                                Arc::clone(&menu_devices),
                                Arc::clone(&blue_info_clone),
                                Arc::clone(&update_menu_event),
                                Arc::clone(&low_battery_devices),
                                &proxy_menu,
                            )
                            .await
                            {
                                eprintln!("Failed to update tray: {e}");
                            }
                        });
                    };
                }
            }
        }

        if let Ok(menu_event) = menu_channel.try_recv() {
            if menu_event.id().as_ref() == "quit" {
                *control_flow = ControlFlow::Exit;
                std::process::exit(0x0100);
            }

            if let Ok(mut config) = config.try_lock() {
                let menu_id = menu_event.id().as_ref();
                // 如菜单ID可以格式化为u64，则菜单事件对应的是更新频率的设置
                if let Ok(update_interval) = menu_id.trim().parse::<u64>() {
                    config.update_interval = update_interval;
                    write_ini_settings(&ini_path, "update_interval", update_interval.to_string());
                    if let Ok(mut update_menu_event) = update_menu_event.lock() {
                        if let Err(err) = proxy_menu.send_event(TrayEvent::ForwardUpdate) {
                            eprintln!("{err}")
                        } else {
                            *update_menu_event = true;
                        }
                    }
                // 如菜单ID可以格式化为f64，则菜单事件对应的是设备低电量的设置
                } else if let Ok(low_battery) = menu_id.trim().parse::<f64>() {
                    let low_battery = (low_battery * 100.0).floor().clamp(0.0, 99.0) as u8;
                    config.notify_low_battery = if low_battery == 0 {
                        None
                    } else {
                        Some(low_battery)
                    };
                    write_ini_settings(&ini_path, "notify_low_battery", low_battery.to_string());
                    if let Ok(mut update_menu_event) = update_menu_event.lock() {
                        if let Err(err) = proxy_menu.send_event(TrayEvent::ForwardUpdate) {
                            eprintln!("{err}");
                        } else {
                            *update_menu_event = true;
                        }
                    }
                } else {
                    match menu_id {
                        "notify_mute" => {
                            config.notify_mute = !config.notify_mute;
                            write_ini_notifications(
                                &ini_path,
                                menu_event.id().as_ref(),
                                config.notify_mute.to_string(),
                            );
                        }
                        "notify_reconnection" => {
                            config.notify_reconnection = !config.notify_reconnection;
                            write_ini_notifications(
                                &ini_path,
                                menu_event.id().as_ref(),
                                config.notify_reconnection.to_string(),
                            );
                        }
                        "notify_disconnection" => {
                            config.notify_disconnection = !config.notify_disconnection;
                            write_ini_notifications(
                                &ini_path,
                                menu_event.id().as_ref(),
                                config.notify_disconnection.to_string(),
                            );
                        }
                        "notify_added_devices" => {
                            config.notify_added_devices = !config.notify_added_devices;
                            write_ini_notifications(
                                &ini_path,
                                menu_event.id().as_ref(),
                                config.notify_added_devices.to_string(),
                            );
                        }
                        "notify_remove_devices" => {
                            config.notify_remove_devices = !config.notify_remove_devices;
                            write_ini_notifications(
                                &ini_path,
                                menu_event.id().as_ref(),
                                config.notify_remove_devices.to_string(),
                            );
                        }
                        "show_disconnected_devices" => {
                            config.show_disconnected_devices = !config.show_disconnected_devices;
                            write_ini_settings(
                                &ini_path,
                                "show_disconnected_devices",
                                config.show_disconnected_devices.to_string(),
                            );
                            if let Ok(mut update_menu_event) = update_menu_event.lock() {
                                if let Err(err) = proxy_menu.send_event(TrayEvent::ForwardUpdate) {
                                    eprintln!("{err}")
                                } else {
                                    *update_menu_event = true;
                                }
                            }
                        }
                        "truncate_device_name" => {
                            config.truncate_device_name = !config.truncate_device_name;
                            write_ini_settings(
                                &ini_path,
                                "truncate_device_name",
                                config.truncate_device_name.to_string(),
                            );
                            if let Ok(mut update_menu_event) = update_menu_event.lock() {
                                if let Err(err) = proxy_menu.send_event(TrayEvent::ForwardUpdate) {
                                    eprintln!("{err}")
                                } else {
                                    *update_menu_event = true;
                                }
                            }
                        }
                        "battery_prefix_name" => {
                            config.battery_prefix_name = !config.battery_prefix_name;
                            write_ini_settings(
                                &ini_path,
                                "battery_prefix_name",
                                config.battery_prefix_name.to_string(),
                            );
                            if let Ok(mut update_menu_event) = update_menu_event.lock() {
                                if let Err(err) = proxy_menu.send_event(TrayEvent::ForwardUpdate) {
                                    eprintln!("{err}")
                                } else {
                                    *update_menu_event = true;
                                }
                            }
                        }
                        "startup" => {
                            let should_startup =
                                !get_startup_status().expect("Failed to get startup status");
                            set_startup(should_startup).expect("Failed to set Launch at Startup")
                        }
                        _ => (),
                    }
                }
            }
        }
    });

    Ok(())
}

fn load_icon(icon_data: &[u8]) -> Result<tray_icon::Icon> {
    let (icon_rgba, icon_width, icon_height) = {
        let image = image::load_from_memory(icon_data)
            .context("Failed to open icon path")?
            .into_rgba8();
        let (width, height) = image.dimensions();
        let rgba = image.into_raw();
        (rgba, width, height)
    };
    tray_icon::Icon::from_rgba(icon_rgba, icon_width, icon_height)
        .context("Failed to crate the logo")
}

async fn update_tray(
    config: Arc<Mutex<Config>>,
    tooltip: Arc<Mutex<Vec<String>>>,
    menu_devices: Arc<Mutex<Vec<String>>>,
    blue_info: Arc<Mutex<HashSet<BluetoothInfo>>>,
    update_menu_event: Arc<Mutex<bool>>,
    low_battery_devices: Arc<Mutex<HashMap<String, bool>>>,
    proxy: &EventLoopProxy<TrayEvent>,
) -> Result<()> {
    let language = Language::get_system_language();
    let loc = Localization::get(language);

    let (current_tooltip, current_menu_devices, current_blue_info) =
        get_bluetooth_tray_info(Arc::clone(&config)).await?;

    let (
        config,
        mut original_tooltip,
        mut original_menu_devices,
        mut original_blue_info,
        mut update_menu_event,
    ) = (
        config
            .try_lock()
            .map_err(|_| anyhow!("Failed to acquire 'config' lock on task"))?,
        tooltip
            .try_lock()
            .map_err(|_| anyhow!("Failed to acquire 'tooltip' lock on task,"))?,
        menu_devices
            .try_lock()
            .map_err(|_| anyhow!("Failed to acquire 'menu_devices' lock on task"))?,
        blue_info
            .try_lock()
            .map_err(|_| anyhow!("Failed to acquire 'blue_info' lock on task"))?,
        update_menu_event
            .try_lock()
            .map_err(|_| anyhow!("Failed to acquire 'update_menu_event' lock on task"))?,
    );

    // 蓝牙信息的集合进行比较时，以HashSet承载信息，与Vec相比，其优势为无需考虑顺序即可比较
    if current_blue_info == *original_blue_info && !*update_menu_event {
        return Ok(());
    }

    let changed_devices = current_blue_info
        .difference(&original_blue_info)
        .collect::<HashSet<_>>();
    let reverted_devices = original_blue_info
        .difference(&current_blue_info)
        .collect::<HashSet<_>>();

    let [
        updated_status_from_current,   // 当前状态改变的设备
        updated_battery_from_current,  // 当前电量改变的设备
        updated_devices_from_current,  // 当前信息改变的设备
        updated_devices_from_reverted, // 既往信息改变的设备
    ]: [HashSet<&BluetoothInfo>; 4] = changed_devices.iter().cloned().fold(
        [
            HashSet::new(),
            HashSet::new(),
            HashSet::new(),
            HashSet::new(),
        ],
        |mut acc, cd| {
            if let Some(rd) = reverted_devices.iter().cloned().find(|rd| cd.id == rd.id) {
                if cd.status != rd.status {
                    acc[0].insert(cd);
                }
                if cd.battery != rd.battery {
                    acc[1].insert(cd);
                }
                if cd.battery != rd.battery || cd.status != rd.status {
                    acc[2].insert(cd);
                    acc[3].insert(rd);
                }
            }
            acc
        },
    );

    if !updated_battery_from_current.is_empty() {
        if let Some(set_battery) = config.notify_low_battery {
            let mut messages = String::new();

            let mut low_battery_devices = low_battery_devices
                .try_lock()
                .map_err(|_| anyhow!("Failed to acquire 'low_battery_devices' lock on task"))?;

            for current_blue_info in updated_battery_from_current {
                // 若设备电量低于阈值，且'low_battery_devices'无记录或有记录但无标记，则标记并发送低电量通知（无需考虑连接状态，因为出现更新了就说明有连接过）
                if current_blue_info.battery < set_battery {
                    let notified = low_battery_devices
                        .entry(current_blue_info.id.clone())
                        .or_insert(false);
                    if !std::mem::replace(&mut *notified, true) {
                        messages.push_str(&format!(
                            "{}: {}%\n",
                            current_blue_info.name, current_blue_info.battery
                        ));
                    }
                // 若设备电量恢复至阈值以上，且'low_battery_devices'有记录及标记，则取消标记允许低电量通知
                } else if let Some(notified) = low_battery_devices.get_mut(&current_blue_info.id) {
                    *notified = false;
                }
            }

            if !messages.is_empty() {
                let title = &format!("{} {set_battery}%", loc.bluetooth_battery_below);
                let text = &messages.trim();
                let mute = config.notify_mute;
                notify(title, text, mute)?
            }
        }
    }

    let Config {
        notify_reconnection,
        notify_disconnection,
        notify_added_devices,
        notify_remove_devices,
        ..
    } = *config;

    if !updated_status_from_current.is_empty() {
        let [reconnection, disconnection] = updated_status_from_current.iter().fold(
            [String::new(), String::new()],
            |[mut reconnection, mut disconnection], current_blue_info| {
                match (
                    current_blue_info.status,
                    notify_reconnection,
                    notify_disconnection,
                ) {
                    (true, true, _) => reconnection.push_str(&format!(
                        "{}: {}\n",
                        loc.device_name, current_blue_info.name
                    )),
                    (false, _, true) => disconnection.push_str(&format!(
                        "{}: {}\n",
                        loc.device_name, current_blue_info.name
                    )),
                    (_, _, _) => (),
                }
                [reconnection, disconnection]
            },
        );
        if notify_reconnection && !reconnection.is_empty() {
            // 重新连接
            notify(
                loc.bluetooth_device_reconnected,
                &reconnection.trim(),
                config.notify_mute,
            )?
        }
        if notify_disconnection && !disconnection.is_empty() {
            // 断开连接
            notify(
                loc.bluetooth_device_disconnected,
                &disconnection.trim(),
                config.notify_mute,
            )?
        }
    }

    // 新添加的设备
    let added_devices = changed_devices
        .difference(&updated_devices_from_current)
        .collect::<HashSet<_>>();
    if !added_devices.is_empty() {
        *update_menu_event = true;
        if notify_added_devices {
            let messeges = added_devices.into_iter().fold(String::new(), |mut acc, b| {
                let name = format!("{}: {}\n", loc.device_name, b.name);
                acc.push_str(&name);
                acc
            });
            notify(
                loc.new_bluetooth_device_connected,
                &messeges.trim(),
                config.notify_mute,
            )?
        }
    }

    // 被移除的设备
    let remove_devices = reverted_devices
        .difference(&updated_devices_from_reverted)
        .collect::<HashSet<_>>();
    if !remove_devices.is_empty() {
        *update_menu_event = true;
        if notify_remove_devices {
            let messeges = remove_devices
                .into_iter()
                .fold(String::new(), |mut acc, b| {
                    let name = format!("{}: {}\n", loc.device_name, b.name);
                    acc.push_str(&name);
                    acc
                });
            notify(
                loc.bluetooth_device_removed,
                &messeges.trim(),
                config.notify_mute,
            )?
        }
    }

    *original_tooltip = current_tooltip;
    *original_blue_info = current_blue_info;

    // 若设备添减或者更改菜单设置，则更新托盘菜单
    if std::mem::replace(&mut *update_menu_event, false) {
        *original_menu_devices = current_menu_devices;
        proxy.send_event(TrayEvent::SetTrayInfo).map_err(|_| {
            anyhow!("Failed to send update tray tooltip and menu events to EventLoop")
        })?;
    } else {
        proxy
            .send_event(TrayEvent::SetTooltip)
            .map_err(|_| anyhow!("Failed to send update tray tooltip event to EventLoop"))?;
    }

    Ok(())
}

async fn get_bluetooth_tray_info(
    config: Arc<Mutex<Config>>,
) -> Result<(Vec<String>, Vec<String>, HashSet<BluetoothInfo>)> {
    let bluetooth_devices = find_bluetooth_devices()
        .await
        .map_err(|e| anyhow!("Failed to find bluetooth devices - {e}"))?;
    let bluetooth_devices_info = get_bluetooth_info(bluetooth_devices.0, bluetooth_devices.1)
        .await
        .map_err(|e| anyhow!("Failed to get bluetooth devices info - {e}"))?;
    let show_disconnected_devices = config.lock().map_or(false, |c| c.show_disconnected_devices);
    let truncate_device_name = config.lock().map_or(false, |c| c.truncate_device_name);
    let battery_prefix_name = config.lock().map_or(false, |c| c.battery_prefix_name);
    let (tooltip, menu_devices) = convert_tray_info(
        &bluetooth_devices_info,
        show_disconnected_devices,
        truncate_device_name,
        battery_prefix_name,
    );
    Ok((tooltip, menu_devices, bluetooth_devices_info))
}

fn convert_tray_info(
    bluetooth_devices_info: &HashSet<BluetoothInfo>,
    show_disconnected_devices: bool,
    truncate_device_name: bool,
    battery_prefix_name: bool,
) -> (Vec<String>, Vec<String>) {
    bluetooth_devices_info
        .iter()
        .fold((Vec::new(), Vec::new()), |mut acc, blue_info| {
            let name = truncate_with_ellipsis(truncate_device_name, &blue_info.name, 10);
            let battery = blue_info.battery;
            let status_icon = if blue_info.status { "🟢" } else { "🔴" };
            let info = if battery_prefix_name {
                format!("{status_icon}{battery:3}% - {name}")
            } else {
                format!("{status_icon}{name} - {battery:3}%")
            };
            match blue_info.status {
                true => {
                    acc.0.insert(0, info);
                    acc.1.insert(0, blue_info.name.to_owned());
                }
                false => {
                    acc.1.push(blue_info.name.to_owned());
                    if show_disconnected_devices {
                        acc.0.push(info);
                    }
                }
            }

            acc
        })
}

fn truncate_with_ellipsis(truncate_device_name: bool, s: &str, max_chars: usize) -> String {
    if truncate_device_name && s.chars().count() > max_chars {
        let mut result = s.chars().take(max_chars).collect::<String>();
        result.push_str("...");
        result
    } else {
        s.to_string()
    }
}

fn create_tray_menu(menu_devices: &Vec<String>, config: &Config) -> Result<Menu> {
    let language = Language::get_system_language();
    let loc = Localization::get(language);

    let tray_menu = Menu::new();

    let menu_separator = PredefinedMenuItem::separator();

    let menu_quit = MenuItem::with_id("quit", loc.exsit, true, None);

    let menu_show_disconnected_devices = CheckMenuItem::with_id(
        "show_disconnected_devices",
        loc.show_disconnected_devices,
        true,
        config.show_disconnected_devices,
        None,
    );

    let menu_truncate_device_name = CheckMenuItem::with_id(
        "truncate_device_name",
        loc.truncate_device_name,
        true,
        config.truncate_device_name,
        None,
    );

    let menu_battery_prefix_name = CheckMenuItem::with_id(
        "battery_prefix_name",
        loc.battery_prefix_name,
        true,
        config.battery_prefix_name,
        None,
    );

    let update_items = &[
        &CheckMenuItem::with_id("15", "15s", true, config.update_interval == 15, None)
            as &dyn IsMenuItem,
        &CheckMenuItem::with_id("30", "30s", true, config.update_interval == 30, None)
            as &dyn IsMenuItem,
        &CheckMenuItem::with_id("60", "1min", true, config.update_interval == 60, None)
            as &dyn IsMenuItem,
        &CheckMenuItem::with_id("300", "5min", true, config.update_interval == 300, None)
            as &dyn IsMenuItem,
        &CheckMenuItem::with_id("600", "10min", true, config.update_interval == 600, None)
            as &dyn IsMenuItem,
        &CheckMenuItem::with_id("1800", "30min", true, config.update_interval == 1800, None)
            as &dyn IsMenuItem,
    ];

    let menu_update =
        Submenu::with_id_and_items("update", loc.update_interval, true, update_items)?;

    let low_battery = config.notify_low_battery;
    let low_battery_items = &[
        &CheckMenuItem::with_id("0.0", loc.none, true, low_battery.is_none(), None)
            as &dyn IsMenuItem,
        &CheckMenuItem::with_id(
            "0.05",
            "5%",
            true,
            low_battery.map_or(false, |v| v == 5),
            None,
        ) as &dyn IsMenuItem,
        &CheckMenuItem::with_id(
            "0.1",
            "10%",
            true,
            low_battery.map_or(false, |v| v == 10),
            None,
        ) as &dyn IsMenuItem,
        &CheckMenuItem::with_id(
            "0.15",
            "15%",
            true,
            low_battery.map_or(false, |v| v == 15),
            None,
        ) as &dyn IsMenuItem,
        &CheckMenuItem::with_id(
            "0.2",
            "20%",
            true,
            low_battery.map_or(false, |v| v == 20),
            None,
        ) as &dyn IsMenuItem,
        &CheckMenuItem::with_id(
            "0.25",
            "25%",
            true,
            low_battery.map_or(false, |v| v == 25),
            None,
        ) as &dyn IsMenuItem,
    ];
    let notify_low_battery = Submenu::with_items(loc.notify_low_battery, true, low_battery_items)?;

    let notify_items = &[
        &CheckMenuItem::with_id(
            "notify_mute",
            loc.notify_mute,
            true,
            config.notify_mute,
            None,
        ) as &dyn IsMenuItem,
        &notify_low_battery as &dyn IsMenuItem,
        &CheckMenuItem::with_id(
            "notify_reconnection",
            loc.notify_reconnection,
            true,
            config.notify_reconnection,
            None,
        ) as &dyn IsMenuItem,
        &CheckMenuItem::with_id(
            "notify_disconnection",
            loc.notify_disconnection,
            true,
            config.notify_disconnection,
            None,
        ) as &dyn IsMenuItem,
        &CheckMenuItem::with_id(
            "notify_added_devices",
            loc.notify_added_devices,
            true,
            config.notify_added_devices,
            None,
        ) as &dyn IsMenuItem,
        &CheckMenuItem::with_id(
            "notify_remove_devices",
            loc.notify_remove_devices,
            true,
            config.notify_remove_devices,
            None,
        ) as &dyn IsMenuItem,
    ];
    let menu_notify = Submenu::with_items(loc.notifications, true, notify_items)?;

    let menu_startup =
        CheckMenuItem::with_id("startup", loc.startup, true, get_startup_status()?, None);

    let settings_items = &[
        &menu_update as &dyn IsMenuItem,
        &menu_notify as &dyn IsMenuItem,
        &menu_startup as &dyn IsMenuItem,
        &menu_show_disconnected_devices as &dyn IsMenuItem,
        &menu_truncate_device_name as &dyn IsMenuItem,
        &menu_battery_prefix_name as &dyn IsMenuItem,
    ];

    let menu_setting = Submenu::with_items(loc.settings, true, settings_items)?;

    let menu_about = PredefinedMenuItem::about(
        Some(loc.about),
        Some(AboutMetadata {
            name: Some("BluetGauge".to_owned()),
            version: Some("0.1.2".to_owned()),
            authors: Some(vec!["iKineticate".to_owned()]),
            website: Some("https://github.com/iKineticate/BlueGauge".to_owned()),
            ..Default::default()
        }),
    );

    for text in menu_devices {
        let item = CheckMenuItem::with_id(text, text, true, false, None);
        tray_menu
            .append(&item)
            .map_err(|_| anyhow!("Failed to append 'Devices' to Tray Menu"))?;
    }

    tray_menu
        .append(&menu_separator)
        .context("Failed to apped 'Separator' to Tray Menu")?;
    tray_menu
        .append(&menu_setting)
        .context("Failed to apped 'Update Interval' to Tray Menu")?;
    tray_menu
        .append(&menu_separator)
        .context("Failed to apped 'Separator' to Tray Menu")?;
    tray_menu
        .append(&menu_about)
        .context("Failed to apped 'About' to Tray Menu")?;
    tray_menu
        .append(&menu_separator)
        .context("Failed to apped 'Separator' to Tray Menu")?;
    tray_menu
        .append(&menu_quit)
        .context("Failed to apped 'Quit' to Tray Menu")?;

    Ok(tray_menu)
}
