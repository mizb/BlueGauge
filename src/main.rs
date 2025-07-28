#![allow(non_snake_case)]
#![cfg(target_os = "windows")]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod bluetooth;
mod config;
mod icon;
mod language;
mod notify;
mod startup;
mod tray;

use crate::bluetooth::{
    BluetoothInfo, compare_bt_info_to_send_notifications, find_bluetooth_devices,
    get_bluetooth_info,
};
use crate::config::*;
use crate::icon::load_battery_icon;
use crate::notify::{app_notify, notify};
use crate::startup::set_startup;
use crate::tray::{create_menu, create_tray};

use std::collections::HashSet;
use std::ops::Deref;
use std::path::Path;
use std::sync::{Arc, Mutex, atomic::Ordering};

use tray_icon::{
    TrayIcon,
    menu::{CheckMenuItem, MenuEvent},
};
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy},
    window::WindowId,
};

fn main() -> anyhow::Result<()> {
    let event_loop = EventLoop::<UserEvent>::with_user_event().build()?;

    let proxy = event_loop.create_proxy();
    MenuEvent::set_event_handler(Some(move |event| {
        proxy
            .send_event(UserEvent::MenuEvent(event))
            .expect("Failed to send MenuEvent");
    }));

    let mut app = App::default();
    let proxy = event_loop.create_proxy();
    app.add_proxy(Some(proxy));

    event_loop.run_app(&mut app)?;

    Ok(())
}

struct App {
    bluetooth_info: Arc<Mutex<HashSet<BluetoothInfo>>>,
    config: Arc<Config>,
    event_loop_proxy: Option<EventLoopProxy<UserEvent>>,
    /// 存储已经通知过的低电量设备，避免再次通知
    notified_low_battery: Arc<Mutex<HashSet<String>>>,
    tray: Mutex<Option<TrayIcon>>,
    tray_check_menus: Mutex<Option<Vec<CheckMenuItem>>>,
}

impl Default for App {
    fn default() -> Self {
        let config = Config::oepn().expect("Failed to open config");

        let (tray, tray_check_menus, bluetooth_info) = create_tray(&config)
            .inspect_err(|e| app_notify(format!("Failed to create tray - {e}")))
            .unwrap();

        Self {
            bluetooth_info: Arc::new(Mutex::new(bluetooth_info)),
            config: Arc::new(config),
            event_loop_proxy: None,
            notified_low_battery: Arc::new(Mutex::new(HashSet::new())),
            tray: Mutex::new(Some(tray)),
            tray_check_menus: Mutex::new(Some(tray_check_menus)),
        }
    }
}

#[derive(Debug)]
enum UserEvent {
    MenuEvent(MenuEvent),
    UpdateTray,
}

impl App {
    fn add_proxy(&mut self, event_loop_proxy: Option<EventLoopProxy<UserEvent>>) -> &mut Self {
        self.event_loop_proxy = event_loop_proxy;
        self
    }
}

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, _event_loop: &ActiveEventLoop) {
        let config = Arc::clone(&self.config);
        let proxy = self
            .event_loop_proxy
            .clone()
            .ok_or("Failed to get proxy")
            .inspect_err(|e| app_notify(e))
            .unwrap();

        std::thread::spawn(move || {
            loop {
                let update_interval = config.get_update_interval();

                for _ in 0..update_interval {
                    std::thread::sleep(std::time::Duration::from_secs(1));
                    if config.force_update.load(Ordering::Acquire) {
                        break;
                    }
                }

                proxy
                    .send_event(UserEvent::UpdateTray)
                    .inspect_err(|e| app_notify(format!("Failed to send UpdateTray Event - {e}")))
                    .unwrap();
            }
        });
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        if event == WindowEvent::CloseRequested {
            event_loop.exit()
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::MenuEvent(event) => {
                let config = Arc::clone(&self.config);
                let tray_check_menus = self
                    .tray_check_menus
                    .lock()
                    .unwrap()
                    .clone()
                    .ok_or("Tray check menus not initialized")
                    .inspect_err(|e| app_notify(e))
                    .unwrap();

                let menu_event_id = event.id().as_ref();
                match menu_event_id {
                    "quit" => event_loop.exit(),
                    "force_update" => config.force_update.store(true, Ordering::Release),
                    "startup" => {
                        if let Some(item) =
                            tray_check_menus.iter().find(|item| item.id() == "startup")
                        {
                            set_startup(item.is_checked())
                                .inspect_err(|e| {
                                    app_notify(format!("Failed to set Launch at Startup - {e}"))
                                })
                                .unwrap()
                        }
                    }
                    // 托盘设置：更新间隔
                    "15" | "30" | "60" | "300" | "600" | "1800" => {
                        // 只处理更新蓝牙信息间隔相关的菜单项
                        let update_interval_items: Vec<_> = tray_check_menus
                            .iter()
                            .filter(|item| {
                                ["15", "30", "60", "300", "600", "1800"]
                                    .contains(&item.id().as_ref())
                            })
                            .collect();

                        // 是否存在被点击且为勾选的项目
                        let is_checked = update_interval_items
                            .iter()
                            .any(|item| item.id().as_ref() == menu_event_id && item.is_checked());

                        // 更新所有菜单项状态
                        update_interval_items.iter().for_each(|item| {
                            let should_check = item.id().as_ref() == menu_event_id && is_checked;
                            item.set_checked(should_check);
                        });

                        // 获取当前勾选的项目对应的电量
                        let selected_update_interval = update_interval_items
                            .iter()
                            .find_map(|item| item.is_checked().then_some(item.id().as_ref()))
                            .and_then(|id| id.parse::<u64>().ok());

                        // 更新配置
                        if let Some(update_interval) = selected_update_interval {
                            config
                                .tray_config
                                .update_interval
                                .store(update_interval, Ordering::Relaxed);
                            config
                                .write_tray_config("update_interval", &update_interval.to_string());
                        } else {
                            let default_update_interval = 30;
                            config
                                .tray_config
                                .update_interval
                                .store(default_update_interval, Ordering::Relaxed);
                            config.write_tray_config(
                                "update_interval",
                                &default_update_interval.to_string(),
                            );

                            // 找到并选中默认项
                            if let Some(default_item) = update_interval_items
                                .iter()
                                .find(|i| i.id().as_ref() == default_update_interval.to_string())
                            {
                                default_item.set_checked(true);
                            }
                        }

                        config.force_update.store(true, Ordering::Release);
                    }
                    // 通知设置：低电量
                    "0.01" | "0.05" | "0.1" | "0.15" | "0.2" | "0.25" => {
                        // 只处理低电量阈值相关的菜单项
                        let low_battery_items: Vec<_> = tray_check_menus
                            .iter()
                            .filter(|item| {
                                ["0.01", "0.05", "0.1", "0.15", "0.2", "0.25"]
                                    .contains(&item.id().as_ref())
                            })
                            .collect();

                        // 是否存在被点击且为勾选的项目
                        let is_checked = low_battery_items
                            .iter()
                            .any(|item| item.id().as_ref() == menu_event_id && item.is_checked());

                        // 更新所有菜单项状态
                        low_battery_items.iter().for_each(|item| {
                            let should_check = item.id().as_ref() == menu_event_id && is_checked;
                            item.set_checked(should_check);
                        });

                        // 获取当前勾选的项目对应的电量
                        let selected_low_battery = low_battery_items
                            .iter()
                            .find(|item| item.is_checked())
                            .and_then(|item| item.id().as_ref().parse::<f64>().ok());

                        // 更新配置
                        if let Some(low_battery) = selected_low_battery {
                            let low_battery = (low_battery * 100.0).round() as u8;
                            config
                                .notify_options
                                .low_battery
                                .store(low_battery, Ordering::Relaxed);
                            config.write_notify_options("low_battery", &low_battery.to_string());
                        } else {
                            let default_low_battery = 15;
                            config
                                .notify_options
                                .low_battery
                                .store(default_low_battery, Ordering::Relaxed);
                            config.write_notify_options(
                                "low_battery",
                                &default_low_battery.to_string(),
                            );

                            // 找到并选中默认项
                            if let Some(default_item) =
                                low_battery_items.iter().find(|i| i.id().as_ref() == "0.15")
                            {
                                default_item.set_checked(true);
                            }
                        }
                    }
                    // 通知设置：静音/断开连接/重新连接/添加/删除
                    "mute" | "disconnection" | "reconnection" | "added" | "removed" => {
                        // 找到对应的菜单（非子菜单），则更新结构体中的配置及配置文件的内容
                        if let Some(item) = tray_check_menus
                            .iter()
                            .find(|item| item.id().as_ref() == menu_event_id)
                        {
                            if item.is_checked() {
                                config.notify_options.update(menu_event_id, true);
                                config.write_notify_options(menu_event_id, "true");
                            } else {
                                config.notify_options.update(menu_event_id, false);
                                config.write_notify_options(menu_event_id, "false");
                            }
                        }
                    }
                    // 托盘设置：提示内容设置
                    "show_disconnected" | "truncate_name" | "prefix_battery" => {
                        if let Some(item) = tray_check_menus
                            .iter()
                            .find(|item| item.id().as_ref() == menu_event_id)
                        {
                            if item.is_checked() {
                                config.tray_config.update(menu_event_id, true);
                                config.write_tray_config(menu_event_id, "true");
                            } else {
                                config.tray_config.update(menu_event_id, false);
                                config.write_tray_config(menu_event_id, "false");
                            }
                        }

                        config.force_update.store(true, Ordering::Release);
                    }
                    _ => {
                        #[rustfmt::skip]
                        let not_bluetooth_item_id = [
                            "quit",
                            "force_update",
                            "startup",
                            "15", "30", "60", "300", "600", "1800",
                            "0.01", "0.05", "0.1",  "0.15", "0.2", "0.25",
                            "mute", "disconnection", "reconnection", "added", "removed",
                            "show_disconnected", "truncate_name", "prefix_battery",
                        ];

                        let show_battery_icon_bt_id = menu_event_id;

                        // 只处理显示蓝牙电量图标相关的菜单项
                        let show_battery_icon_items: Vec<_> = tray_check_menus
                            .iter()
                            .filter(|item| !not_bluetooth_item_id.contains(&item.id().as_ref()))
                            .collect();

                        let is_checked = show_battery_icon_items.iter().any(|item| {
                            item.id().as_ref() == show_battery_icon_bt_id && item.is_checked()
                        });

                        show_battery_icon_items.iter().for_each(|item| {
                            let should_check =
                                item.id().as_ref() == show_battery_icon_bt_id && is_checked;
                            item.set_checked(should_check);
                        });

                        let mut tray_icon_source =
                            config.tray_config.tray_icon_source.lock().unwrap();

                        match tray_icon_source.deref() {
                            TrayIconSource::App if is_checked => {
                                let have_custom_icons = std::env::current_exe()
                                    .ok()
                                    .and_then(|exe_path| exe_path.parent().map(Path::to_path_buf))
                                    .map(|p| {
                                        (0..=100)
                                            .all(|i| p.join(format!("assets\\{i}.png")).is_file())
                                    })
                                    .unwrap_or(false);

                                *tray_icon_source = if have_custom_icons {
                                    TrayIconSource::BatteryCustom(
                                        show_battery_icon_bt_id.to_string(),
                                    )
                                } else {
                                    TrayIconSource::BatteryDefault(
                                        show_battery_icon_bt_id.to_string(),
                                    )
                                };

                                config
                                    .write_tray_config("tray_icon_source", show_battery_icon_bt_id);
                            }
                            TrayIconSource::BatteryCustom(_)
                            | TrayIconSource::BatteryDefault(_) => {
                                if is_checked {
                                    tray_icon_source.update_id(show_battery_icon_bt_id);
                                    config.write_tray_config(
                                        "tray_icon_source",
                                        show_battery_icon_bt_id,
                                    );
                                } else {
                                    *tray_icon_source = TrayIconSource::App;
                                    config.write_tray_config("tray_icon_source", "");
                                }
                            }
                            _ => (),
                        }
                        config.force_update.store(true, Ordering::Release);
                    }
                }
            }
            UserEvent::UpdateTray => {
                let bluetooth_devices = match find_bluetooth_devices() {
                    Ok(devices) => devices,
                    Err(e) => {
                        app_notify(format!("Failed to find bluetooth devices - {e}"));
                        return;
                    }
                };

                let new_bt_info = match get_bluetooth_info(bluetooth_devices) {
                    Ok(infos) => infos,
                    Err(e) => {
                        app_notify(format!("Failed to get bluetooth devices info - {e}"));
                        return;
                    }
                };

                let config = Arc::clone(&self.config);

                if let Some(e) = compare_bt_info_to_send_notifications(
                    &config,
                    Arc::clone(&self.notified_low_battery),
                    Arc::clone(&self.bluetooth_info),
                    &new_bt_info,
                ) {
                    e.inspect_err(|e| {
                        app_notify(format!("Failed to compare bluetooth info - {e}"))
                    })
                    .expect("Failed to compare bluetooth info");
                } else {
                    // 避免菜单事件使配置更新后，因蓝牙信息无更新而不执行后续更新代码
                    if !config.force_update.swap(false, Ordering::Acquire) {
                        return;
                    }
                }

                let (tray_menu, new_tray_check_menus, tooltip, _) = create_menu(&config)
                    .inspect_err(|e| app_notify(format!("Failed to create tray menu - {e}")))
                    .unwrap();

                if let Some(tray) = &self.tray.lock().unwrap().as_mut() {
                    let icon = load_battery_icon(&config, &new_bt_info)
                        .expect("Failed to load battery icon");
                    tray.set_menu(Some(Box::new(tray_menu)));
                    tray.set_tooltip(Some(tooltip.join("\n")))
                        .expect("Failed to update tray tooltip");
                    tray.set_icon(Some(icon)).expect("Failed to set tray icon");
                }

                if let Some(tray_check_menus) = self.tray_check_menus.lock().unwrap().as_mut() {
                    *tray_check_menus = new_tray_check_menus;
                }
            }
        }
    }
}
