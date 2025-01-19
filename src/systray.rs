use std::sync::{Arc, Mutex};
use std::time::Duration;
use std::collections::HashSet;

use image;
use tao::{
    event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy},
    platform::run_return::EventLoopExtRunReturn,
};
use tray_icon::menu::{AboutMetadata, CheckMenuItem, IsMenuItem, Submenu};
use tray_icon::{
    menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem},
    TrayIconBuilder,
};
use anyhow::{Result, Context, anyhow};

use crate::bluetooth::{find_bluetooth_devices, get_bluetooth_info, BluetoothInfo};
use crate::config::*;
use crate::notify::notify;

const ICON_DATA: &[u8] = include_bytes!("../resources/logo.ico");

// enum MenuEvents {
//     Update,
//     Notify,
// }

pub async fn show_systray() -> Result<()> {
    loop_systray().await
}

async fn loop_systray() -> Result<()> {
    let (ini, ini_path) = ini()?;

    let mut event_loop = EventLoopBuilder::<bool>::with_user_event().build();
    let event_loop_proxy = event_loop.create_proxy();

    let (tooltip, menu_devices, blue_info) =
        get_bluetooth_tray_info(Arc::new(Mutex::new(ini.clone()))).await?;
    let tray_menu = create_tray_menu(&menu_devices, &ini)?;
    let tray_icon = TrayIconBuilder::new()
        .with_menu_on_left_click(true)
        .with_icon(load_icon(ICON_DATA).map_err(|e| anyhow!("Failed to load icon - {e}"))?)
        .with_tooltip(tooltip.join("\n"))
        .with_menu(Box::new(tray_menu))
        .build()
        .context("Failed to build tray")?;

    let config = Arc::new(Mutex::new(ini));
    let tooltip = Arc::new(Mutex::new(tooltip));
    let menu_devices = Arc::new(Mutex::new(menu_devices));
    let blue_info = Arc::new(Mutex::new(blue_info));
    let should_update_menu = Arc::new(Mutex::new(false));

    let config_clone = Arc::clone(&config);
    let tooltip_clone = Arc::clone(&tooltip);
    let menu_devices_clone = Arc::clone(&menu_devices);
    let blue_info_clone = Arc::clone(&blue_info);
    let should_update_menu_clone = Arc::clone(&should_update_menu);
    tokio::task::spawn(async move {
        loop {
            let update_interval = {
                let config = config_clone.lock().unwrap();
                config.update_interval
            };

            tokio::time::sleep(tokio::time::Duration::from_secs(update_interval)).await;

            let tooltip = Arc::clone(&tooltip_clone);
            let menu_devices = Arc::clone(&menu_devices_clone);
            let blue_info = Arc::clone(&blue_info_clone);
            let config = Arc::clone(&config_clone);
            let should_update_menu = Arc::clone(&should_update_menu_clone);

            if let Err(e) = update_tray(
                tooltip, menu_devices, blue_info, config, should_update_menu, &event_loop_proxy
            ).await {
                println!("{e}")
            }
        }
    });


    let menu_channel = MenuEvent::receiver();

    let config_clone = Arc::clone(&config);
    let tooltip_clone = Arc::clone(&tooltip);
    let menu_devices_clone = Arc::clone(&menu_devices);
    let should_update_menu_clone = Arc::clone(&should_update_menu);

    let return_code = event_loop.run_return(|event, _, control_flow| {
        *control_flow =
            ControlFlow::WaitUntil(std::time::Instant::now() + Duration::from_millis(100));

        if let tao::event::Event::UserEvent(should_update_menu) = event {
            if let Ok(t) = tooltip_clone.lock() {
                tray_icon.set_tooltip(Some(t.join("\n"))).expect("Failed to update tray tooltip");
            } else {
                println!("Failed to acquire 'tooltip' lock, doing something else")
            }

            if should_update_menu {
                match (menu_devices_clone.lock(), config_clone.lock()) {
                    (Ok(menu_devices), Ok(config)) => {
                        if let Ok(tray_menu) = create_tray_menu(&menu_devices, &config) {
                            tray_icon.set_menu(Some(Box::new(tray_menu)));
                        } else {
                            println!("Failed to update(set) tray menu")
                        }
                    },
                    (_, _) => println!("Failed to acquire 'menu_devices' or 'config' lock, doing something else")
                }
            }
        }

        if let Ok(menu_event) = menu_channel.try_recv() {
            if menu_event.id().as_ref() == "quit"{
                *control_flow = ControlFlow::Exit
            }

            if let Ok(mut config) = config_clone.try_lock() {
                let id = menu_event.id().as_ref();
                // 如果id是数字，即表示为更新频率时间
                if let Ok(update_interval) = id.trim().parse::<u64>() {
                    // 记得检查菜单此时是被check还是uncheck
                    config.update_interval = update_interval;
                    write_ini_update_interval(&ini_path, update_interval);
                    if let Ok(mut should_update_menu) = should_update_menu_clone.lock() {
                        *should_update_menu = true
                    }
                } else {
                    match id {
                        "notify_mute" => {
                            config.notify_mute = !config.notify_mute;
                            write_ini_notifications(&ini_path, menu_event.id().as_ref(), config.notify_mute.to_string());
                        },
                        "notify_reconnection" => {
                            config.notify_reconnection = !config.notify_reconnection;
                            write_ini_notifications(&ini_path, menu_event.id().as_ref(), config.notify_reconnection.to_string());
                        },
                        "notify_disconnection" => {
                            config.notify_disconnection = !config.notify_disconnection;
                            write_ini_notifications(&ini_path, menu_event.id().as_ref(), config.notify_disconnection.to_string());
                        },
                        "notify_added_devices" => {
                            config.notify_added_devices = !config.notify_added_devices;
                            write_ini_notifications(&ini_path, menu_event.id().as_ref(), config.notify_added_devices.to_string());
                        },
                        "notify_remove_devices" => {
                            config.notify_remove_devices = !config.notify_remove_devices;
                            write_ini_notifications(&ini_path, menu_event.id().as_ref(), config.notify_remove_devices.to_string());
                        },
                        "notify_low_battery" => {
                            if config.notify_low_battery.is_some() {
                                config.notify_low_battery = None;
                                write_ini_notify_low_battery(&ini_path, "none");
                            } else {
                                // 自行设置config.ini
                                config.notify_low_battery = Some(15);
                                write_ini_notify_low_battery(&ini_path, "15");
                            }
                        },
                        "show_disconnected_devices" => {
                            config.show_disconnected_devices = !config.show_disconnected_devices;
                            write_ini_show_disconnected(&ini_path, config.show_disconnected_devices.to_string());
                            if let Ok(mut should_update_menu) = should_update_menu_clone.lock() {
                                *should_update_menu = true
                            }
                        },
                        _ => ()
                    }
                }
            }
        };
    });

    if return_code != 0 {
        std::process::exit(return_code);
    };

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
    tray_icon::Icon::from_rgba(icon_rgba, icon_width, icon_height).context("Failed to crate the logo")
}

async fn get_bluetooth_tray_info(config: Arc<Mutex<Config>>) -> Result<(Vec<String>, Vec<String>, HashSet<BluetoothInfo>)> {
    let bluetooth_devices = find_bluetooth_devices()
        .await
        .map_err(|e| anyhow!("Failed to find bluetooth devices - {e}"))?;
    let bluetooth_devices_info = get_bluetooth_info(bluetooth_devices.0, bluetooth_devices.1)
        .await
        .map_err(|e| anyhow!("Failed to get bluetooth devices info - {e}"))?;
    let show_disconnected_devices = config.lock().map_or(false, |c| c.show_disconnected_devices);
    let (tooltip, menu_devices) = convert_tray_info(&bluetooth_devices_info, show_disconnected_devices);
    Ok((tooltip, menu_devices, bluetooth_devices_info))
}

fn convert_tray_info(
    bluetooth_devices_info: &HashSet<BluetoothInfo>,
    show_disconnected_devices: bool,
) -> (Vec<String>, Vec<String>) {
    bluetooth_devices_info.iter().fold((Vec::new(), Vec::new()), |mut acc, blue_info| {
        let name = truncate_with_ellipsis(&blue_info.name, 10);
        let battery = blue_info.battery;
        let status_icon = if blue_info.status { "🟢" } else { "🔴" }; // { "[●]" } else { "[−]" }
        let info = format!("{status_icon}{battery:3}% - {name}");
        match blue_info.status {
            true => {
                acc.0.insert(0, info);
                acc.1.insert(0, blue_info.name.to_owned());
            },
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

fn truncate_with_ellipsis(s: &str, max_chars: usize) -> String {
    if s.chars().count() > max_chars {
        let mut result = s.chars().take(max_chars).collect::<String>();
        result.push_str("...");
        result
    } else {
        s.to_string()
    }
}

async fn update_tray(
    tooltip: Arc<Mutex<Vec<String>>>,
    menu_devices: Arc<Mutex<Vec<String>>>,
    blue_info: Arc<Mutex<HashSet<BluetoothInfo>>>,
    config: Arc<Mutex<Config>>,
    should_update_menu: Arc<Mutex<bool>>,
    proxy: &EventLoopProxy<bool>
) -> Result<()> {
    let config_clone = Arc::clone(&config);
    let (current_tooltip, current_menu_devices, current_blue_info) = 
        get_bluetooth_tray_info(config_clone).await?;

    let (
        mut original_tooltip,
        mut original_menu_devices,
        mut original_blue_info,
        mut should_update_menu,
        config
    ) = (
        tooltip.try_lock().map_err(|_| anyhow!("Failed to acquire 'tooltip' lock on task, doing something else."))?,
        menu_devices.try_lock().map_err(|_| anyhow!("Failed to acquire 'menu_devices' lock on task, doing something else."))?,
        blue_info.try_lock().map_err(|_| anyhow!("Failed to acquire 'blue_info' lock on task, doing something else."))?,
        should_update_menu.try_lock().map_err(|_| anyhow!("Failed to acquire 'should_update_menu' lock on task, doing something else."))?,
        config.try_lock().map_err(|_| anyhow!("Failed to acquire 'config' lock on task, doing something else."))?
    );

    // 标记已经通知的软件，除非这个软件电量恢复正常后取消标记
    if let Some(battery) = config.notify_low_battery {
        let messeges = current_blue_info.iter().fold(String::new(), |mut acc, blue_info| {
            if blue_info.battery < battery {
                let name = format!("Device Name: {}\n", blue_info.name);
                acc.push_str(&name);
            }
            acc
        });

        if !messeges.is_empty() {
            notify(
                &format!("Bluetooth Battery Below {battery}%"),
                &messeges.trim(),
                config.notify_mute
            )?
        }
    }

    // 蓝牙信息的集合进行比较时，HashSet无需考虑顺序进行比较
    if current_blue_info != *original_blue_info || *should_update_menu {
        let (
            notify_reconnection,
            notify_disconnection,
            notify_added_devices,
            notify_remove_devices
        ) = (
            config.notify_reconnection,
            config.notify_disconnection,
            config.notify_added_devices,
            config.notify_remove_devices
        );

        let changed_devices = current_blue_info.difference(&original_blue_info).collect::<HashSet<_>>();
        let reverted_devices = original_blue_info.difference(&current_blue_info).collect::<HashSet<_>>();
        let mut updated_devices_from_current = HashSet::new();
        let mut updated_devices_from_reverted = HashSet::new();
        for changed_device in changed_devices.clone() {
            for reverted_device in reverted_devices.clone() {
                if changed_device.id == reverted_device.id {
                    if changed_device.status != reverted_device.status || changed_device.battery != reverted_device.battery {
                        updated_devices_from_current.insert(changed_device);
                        updated_devices_from_reverted.insert(reverted_device);
                    }
                }
            }
        }

        if updated_devices_from_current.len() > 0 {
            let [reconnection, disconnection] = updated_devices_from_current
                .clone()
                .into_iter()
                .fold([String::new(), String::new()], |mut acc, blue_info| {
                    let name = format!("Device Name: {}\n", blue_info.name);
                    match (blue_info.status, notify_reconnection, notify_disconnection) {
                        (true, true, _) => acc[0].push_str(&name),
                        (false, _, true) => acc[1].push_str(&name),
                        (_, _, _) => ()
                    }
                    acc
                });
            if notify_reconnection && !reconnection.is_empty() { // 重新连接
                notify("Bluetooth Device Reconnected", &reconnection.trim(), config.notify_mute)?
            }
            if notify_disconnection && !disconnection.is_empty() { // 断开连接
                notify("Bluetooth Device Disconnected", &disconnection.trim(), config.notify_mute)?
            }
        }

        let added_devices = changed_devices.difference(&updated_devices_from_current).collect::<HashSet<_>>();
        let remove_devices = reverted_devices.difference(&updated_devices_from_reverted).collect::<HashSet<_>>();
        let added_devices_len = added_devices.len();
        let remove_devices_len = remove_devices.len();

        if added_devices_len > 0 {
            *should_update_menu = true;
            if notify_added_devices { // 新设备被添加
                let messeges = added_devices.into_iter().fold(String::new(), |mut acc, b| {
                    let name = format!("Device Name: {}\n", b.name);
                    acc.push_str(&name);
                    acc
                });
                notify("New Bluetooth Device Connected", &messeges.trim(), config.notify_mute)?
            }
        }

        if remove_devices_len > 0 {
            *should_update_menu = true;
            if notify_remove_devices { // 设备被删除
                let messeges = remove_devices.into_iter().fold(String::new(), |mut acc, b| {
                    let name = format!("Device Name: {}\n", b.name);
                    acc.push_str(&name);
                    acc
                });
                notify("Bluetooth Device Removed", &messeges.trim(), config.notify_mute)?
            }
        }

        *original_tooltip = current_tooltip;
        *original_blue_info = current_blue_info;

        // 若设备添减或者更改指定设置，则更新托盘菜单
        if *should_update_menu {
            *original_menu_devices = current_menu_devices;
            *should_update_menu = false;
            proxy.send_event(true).context("Failed to send update tray tooltip and menu events to EventLoop")?;
        } else {
            proxy.send_event(false).context("Failed to send update tray tooltip and menu events to EventLoop")?;
        }
    }

    Ok(())
}

fn create_tray_menu(menu_devices: &Vec<String>, config: &Config) -> Result<Menu> {
    let tray_menu = Menu::new();

    let menu_separator = PredefinedMenuItem::separator();

    let menu_quit = MenuItem::with_id("quit", "Quit", true, None);

    let menu_show_disconnected_devices = CheckMenuItem::with_id(
        "show_disconnected_devices",
        "Show Disconnected", 
        true,
        config.show_disconnected_devices,
        None
    );

    let update_items = &[
        &CheckMenuItem::with_id("15", "15s", true, config.update_interval == 15, None) as &dyn IsMenuItem,
        &CheckMenuItem::with_id("30", "30s", true, config.update_interval == 30, None) as &dyn IsMenuItem,
        &CheckMenuItem::with_id("60", "1min", true, config.update_interval == 60, None) as &dyn IsMenuItem,
        &CheckMenuItem::with_id("300", "5min", true, config.update_interval == 300, None) as &dyn IsMenuItem,
        &CheckMenuItem::with_id("600", "10min", true, config.update_interval == 600, None) as &dyn IsMenuItem,
        &CheckMenuItem::with_id("1800", "30min", true, config.update_interval == 1800, None) as &dyn IsMenuItem,
    ];

    let menu_update = Submenu::with_id_and_items(
        "update",
        "Update Interval",
        true,
        update_items,
    )?;

    let notify_low_battery = config.notify_low_battery.map_or(false, |battery| battery < 100);

    let notify_items = &[
        &CheckMenuItem::with_id("notify_mute", "Notify Silently", true, config.notify_mute, None) as &dyn IsMenuItem,
        &CheckMenuItem::with_id("notify_low_battery", "Low Battery", true, notify_low_battery, None) as &dyn IsMenuItem,
        &CheckMenuItem::with_id("notify_reconnection", "Reconnection", true, config.notify_reconnection, None) as &dyn IsMenuItem,
        &CheckMenuItem::with_id("notify_disconnection", "Disconnection", true, config.notify_disconnection, None) as &dyn IsMenuItem,
        &CheckMenuItem::with_id("notify_added_devices", "New Devices", true, config.notify_added_devices, None) as &dyn IsMenuItem,
        &CheckMenuItem::with_id("notify_remove_devices", "Remove Devices", true, config.notify_remove_devices, None) as &dyn IsMenuItem,
    ];

    let menu_notify = Submenu::with_items(
        "Notifications",
        true,
        notify_items,
    )?;

    let menu_about = PredefinedMenuItem::about(
        Some("About"),
        Some(AboutMetadata {
            name: Some("BluetGauge".to_owned()),
            version: Some("0.1.2".to_owned()),
            authors: Some(vec!["iKineticate".to_owned()]),
            website: Some("https://github.com/iKineticate/BlueGauge".to_owned()),
            ..Default::default()
        }));

    menu_devices.iter().for_each(|text| {
        let item = CheckMenuItem::with_id(text, text, true, false, None);
        tray_menu.append(&item).unwrap();
    });
    tray_menu.append(&menu_separator).context("Failed to apped 'Separator' to Tray Menu")?;
    tray_menu.append(&menu_show_disconnected_devices).context("Failed to apped 'Separator' to Tray Menu")?;
    tray_menu.append(&menu_separator).context("Failed to apped 'Separator' to Tray Menu")?;
    tray_menu.append(&menu_update).context("Failed to apped 'Update Interval' to Tray Menu")?;
    tray_menu.append(&menu_separator).context("Failed to apped 'Separator' to Tray Menu")?;
    tray_menu.append(&menu_notify).context("Failed to apped 'Update Interval' to Tray Menu")?;
    tray_menu.append(&menu_separator).context("Failed to apped 'Separator' to Tray Menu")?;
    tray_menu.append(&menu_about).context("Failed to apped 'About' to Tray Menu")?;
    tray_menu.append(&menu_quit).context("Failed to apped 'Quit' to Tray Menu")?;
    Ok(tray_menu)
}