use std::sync::{Arc, Mutex};
use std::time::Duration;
use std::collections::HashSet;

use image;
use tao::{
    event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy},
    platform::run_return::EventLoopExtRunReturn,
};
use tray_icon::menu::{AboutMetadata, CheckMenuItem, Submenu};
use tray_icon::{
    menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem},
    TrayIconBuilder,
};
use anyhow::{Result, Context, anyhow};

use crate::bluetooth::{find_bluetooth_devices, get_bluetooth_info, BluetoothInfo};
use crate::config::{ini, Config};
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
    let config = Arc::new(Mutex::new(ini()?));

    let mut event_loop = EventLoopBuilder::<bool>::with_user_event().build();
    let event_loop_proxy = event_loop.create_proxy();

    let (tooltip, menu, blue_info) = get_bluetooth_tray_info().await?;
    let tray_menu = create_tray_menu(&menu)?;
    let tray_icon = TrayIconBuilder::new()
        .with_menu_on_left_click(true)
        .with_icon(load_icon(ICON_DATA).map_err(|e| anyhow!("Failed to load icon - {e}"))?)
        .with_tooltip(tooltip.join("\n"))
        .with_menu(Box::new(tray_menu))
        .build()
        .context("Failed to build tray")?;
        
    let tooltip = Arc::new(Mutex::new(tooltip));
    let menu = Arc::new(Mutex::new(menu));
    let blue_info = Arc::new(Mutex::new(blue_info));

    let config_clone = Arc::clone(&config);
    let tooltip_clone = Arc::clone(&tooltip);
    let menu_clone = Arc::clone(&menu);
    let blue_info_clone = Arc::clone(&blue_info);
    tokio::task::spawn(async move {
        loop {
            let seconds = {
                let config = config_clone.lock().unwrap();
                config.update_interval
            };
            tokio::time::sleep(tokio::time::Duration::from_secs(seconds)).await;
            let tooltip = Arc::clone(&tooltip_clone);
            let menu = Arc::clone(&menu_clone);
            let config = Arc::clone(&config_clone);
            let blue_info = Arc::clone(&blue_info_clone);
            if let Err(e) = update_tray(tooltip, menu, blue_info, config, &event_loop_proxy).await {
                println!("{e}")
            }
        }
    });

    let menu_channel = MenuEvent::receiver();
    let tooltip_clone = Arc::clone(&tooltip);
    let menu_clone = Arc::clone(&menu);

    let return_code = event_loop.run_return(|event, _, control_flow| {
        *control_flow =
            ControlFlow::WaitUntil(std::time::Instant::now() + Duration::from_millis(100));

        match event {
            tao::event::Event::UserEvent(update_menu) => {
                if let Ok(t) = tooltip_clone.try_lock() {
                    tray_icon.set_tooltip(Some(t.join("\n"))).expect("Failed to update tray tooltip");
                }

                if update_menu {
                    if let Ok(menu) = menu_clone.try_lock() {
                        if let Ok(tray_menu) = create_tray_menu(&menu) {
                            tray_icon.set_menu(Some(Box::new(tray_menu)));
                        }
                    }
                }
            }
            _ => (),
        };

        if let Ok(menu_event) = menu_channel.try_recv() {
            match menu_event.id().as_ref() {
                "quit" => *control_flow = ControlFlow::Exit,
                _ => ()
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

async fn get_bluetooth_tray_info() -> Result<(Vec<String>, Vec<String>, Vec<BluetoothInfo>)> {
    let bluetooth_devices = find_bluetooth_devices()
        .await
        .map_err(|e| anyhow!("Failed to find bluetooth devices - {e}"))?;
    let bluetooth_devices_info = get_bluetooth_info(bluetooth_devices.0, bluetooth_devices.1)
        .await
        .map_err(|e| anyhow!("Failed to get bluetooth devices info - {e}"))?;
    let (tooltip, menu) = convert_tray_info(&bluetooth_devices_info);
    Ok((tooltip, menu, bluetooth_devices_info))
}

fn convert_tray_info(bluetooth_devices_info: &[BluetoothInfo]) -> (Vec<String>, Vec<String>) {
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
                acc.0.push(info);
                acc.1.push(blue_info.name.to_owned());
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
    menu: Arc<Mutex<Vec<String>>>,
    blue_info: Arc<Mutex<Vec<BluetoothInfo>>>,
    config: Arc<Mutex<Config>>,
    proxy: &EventLoopProxy<bool>
) -> Result<()> {
    let (new_tooltip, new_menu, new_blue_info) = get_bluetooth_tray_info().await?;
    let (mut tooltip, mut menu, mut blue_info, config) = (
        tooltip.try_lock().map_err(|_| anyhow!("Failed to acquire 'tooltip' lock on task"))?,
        menu.try_lock().map_err(|_| anyhow!("Failed to acquire 'menu' lock on task"))?,
        blue_info.try_lock().map_err(|_| anyhow!("Failed to acquire 'blue_info' lock on task"))?,
        config.try_lock().map_err(|_| anyhow!("Failed to acquire 'config' lock on task"))?
    );

    let current_blue_info = new_blue_info.iter().cloned().collect::<HashSet<_>>();
    let original_blue_info = blue_info.iter().cloned().collect::<HashSet<_>>();

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
            notify("Bluetooth devices with less than 30% power", &messeges.trim(), config.notify_mute)?
        }
    }

    // 两个HashSet进行比较时无需考虑顺序，而Vec需要考虑顺序问题
    if current_blue_info != original_blue_info {
        let changed_devices = current_blue_info.difference(&original_blue_info).collect::<HashSet<_>>();
        let reverted_devices = original_blue_info.difference(&current_blue_info).collect::<HashSet<_>>();

        match (config.notify_reconnection, config.notify_disconnection, config.notify_new_devices, config.notify_remove_devices) {
            (false, false, false, false) => (),
            (
                notify_reconnection,
                notify_disconnection,
                notify_new_devices,
                notify_remove_devices,
            ) => {
                let mut updated_devices_from_current = HashSet::new();
                let mut updated_devices_from_reverted = HashSet::new();

                for changed_device in changed_devices.clone() {
                    for reverted_device in reverted_devices.clone() {
                        if changed_device.id == reverted_device.id && changed_device.status != reverted_device.status {
                            updated_devices_from_current.insert(changed_device);
                            updated_devices_from_reverted.insert(reverted_device);
                        }
                    }
                }

                if updated_devices_from_current.len() > 0 {
                    let [reconnection, disconnection] = updated_devices_from_current.clone()
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

                if notify_new_devices { // 新设备被添加
                    let added_devices = changed_devices.difference(&updated_devices_from_current).collect::<HashSet<_>>();
                    if added_devices.len() > 0 {
                        let messeges = added_devices.into_iter().fold(String::new(), |mut acc, b| {
                            let name = format!("Device Name: {}\n", b.name);
                            acc.push_str(&name);
                            acc
                        });
                        notify("New Bluetooth Device Connected", &messeges.trim(), config.notify_mute)?
                    }
                }

                if notify_remove_devices { // 设备被移除
                    let remove_devices = reverted_devices.difference(&updated_devices_from_reverted).collect::<HashSet<_>>();
                    if remove_devices.len() > 0 {
                        let messeges = remove_devices.into_iter().fold(String::new(), |mut acc, b| {
                            let name = format!("Device Name: {}\n", b.name);
                            acc.push_str(&name);
                            acc
                        });
                        println!("有蓝牙设备被移除");
                        notify("Bluetooth Device Removed", &messeges.trim(), config.notify_mute)?
                    }
                }
            }
        }

        *tooltip = new_tooltip;
        *menu = new_menu;
        *blue_info = new_blue_info;
        proxy.send_event(true).context("Failed to send update tray tooltip and menu events to EventLoop")?;
    };

    Ok(())
}

fn create_tray_menu(menu: &Vec<String>) -> Result<Menu> {
    let tray_menu = Menu::new();

    let menu_separator = PredefinedMenuItem::separator();
    let menu_update = Submenu::with_id_and_items(
        "update",
        "Update Interval",
        true,
        &[
            &CheckMenuItem::with_id("time_10s", "10s", true, false, None),
            &CheckMenuItem::with_id("time_30s", "30s", true, true, None),
            &CheckMenuItem::with_id("time_1min", "1min", true, false, None),
            &CheckMenuItem::with_id("time_5min", "5min", true, false, None),
            &CheckMenuItem::with_id("time_10min", "10min", true, false, None),
            &CheckMenuItem::with_id("time_30min", "30min", true, false, None),
        ],
    )?;
    let menu_notify = Submenu::with_items(
        "Notifications",
        true,
        &[
            &CheckMenuItem::with_id("notify_low_battery", "Low Battery", true, false, None),
            &CheckMenuItem::with_id("notify_reconnection", "Reconnection", true, false, None),
            &CheckMenuItem::with_id("notify_disconnection", "Disconnection", true, false, None),
            &CheckMenuItem::with_id("notify_new_devices", "New Device", true, false, None),
            &CheckMenuItem::with_id("notify_mute", "Notify Silently", true, true, None),
        ],
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
    let menu_quit = MenuItem::with_id("quit", "Quit", true, None);

    menu.iter().for_each(|text| {
        let item = CheckMenuItem::with_id(text, text, true, false, None);
        tray_menu.append(&item).unwrap();
    });
    tray_menu.append(&menu_separator).context("Failed to apped 'Separator' to Tray Menu")?;
    tray_menu.append(&menu_update).context("Failed to apped 'Update Interval' to Tray Menu")?;
    tray_menu.append(&menu_separator).context("Failed to apped 'Separator' to Tray Menu")?;
    tray_menu.append(&menu_notify).context("Failed to apped 'Update Interval' to Tray Menu")?;
    tray_menu.append(&menu_separator).context("Failed to apped 'Separator' to Tray Menu")?;
    tray_menu.append(&menu_about).context("Failed to apped 'About' to Tray Menu")?;
    tray_menu.append(&menu_quit).context("Failed to apped 'Quit' to Tray Menu")?;
    Ok(tray_menu)
}