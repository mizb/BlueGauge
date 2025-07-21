#![allow(non_snake_case)]
#![cfg(target_os = "windows")]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod bluetooth;
mod config;
mod language;
mod notify;
mod startup;
mod tray;

use crate::bluetooth::{
    BluetoothInfo, compare_bt_info_to_send_notifications, find_bluetooth_devices,
    get_bluetooth_info,
};
use crate::config::*;
use crate::notify::notify;
use crate::startup::set_startup;
use crate::tray::{create_menu, create_tray};

use std::collections::{HashMap, HashSet};
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

const ICON_DATA: &[u8] = include_bytes!("../assets/logo.ico");

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
    config: Arc<Mutex<Config>>,
    event_loop_proxy: Option<EventLoopProxy<UserEvent>>,
    low_battery_devices: Arc<Mutex<HashMap<String, bool>>>,
    tray: Mutex<Option<TrayIcon>>,
    tray_check_menus: Mutex<Option<Vec<CheckMenuItem>>>,
}

impl Default for App {
    fn default() -> Self {
        let config = Config::oepn().expect("Failed to open config");

        let (tray, tray_check_menus, bluetooth_info) =
            create_tray(&config).expect("Failed to create tray");

        Self {
            bluetooth_info: Arc::new(Mutex::new(bluetooth_info)),
            config: Arc::new(Mutex::new(config)),
            event_loop_proxy: None,
            low_battery_devices: Arc::new(Mutex::new(HashMap::new())),
            tray: Mutex::new(Some(tray)),
            tray_check_menus: Mutex::new(Some(tray_check_menus)),
        }
    }
}

#[derive(Debug)]
enum UserEvent {
    MenuEvent(MenuEvent),
    UpdateTray,
    UpdateTrayIcon(tray_icon::Icon),
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
        let proxy = self.event_loop_proxy.clone().expect("Failed to get proxy");

        std::thread::spawn(move || {
            loop {
                // 使用闭包让锁自动释放，避免在等待过程中锁死Config
                let update_interval = {
                    let config = config.lock().unwrap();
                    config.get_update_interval()
                };
                std::thread::sleep(std::time::Duration::from_secs(update_interval));

                {
                    let config = config.lock().unwrap();
                    if config
                        .tray_config
                        .updated_in_advance
                        .swap(false, Ordering::SeqCst)
                    {
                        continue;
                    }
                }

                proxy
                    .send_event(UserEvent::UpdateTray)
                    .expect("Failed to send UpdateTray Event");
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
                let mut config = self.config.lock().unwrap();
                let tray_check_menus = self
                    .tray_check_menus
                    .lock()
                    .unwrap()
                    .clone()
                    .expect("Tray check menus not initialized");

                let menu_event_id = event.id().as_ref();
                match menu_event_id {
                    "quit" => event_loop.exit(),
                    "startup" => {
                        if let Some(item) =
                            tray_check_menus.iter().find(|item| item.id() == "startup")
                        {
                            set_startup(item.is_checked()).expect("Failed to set Launch at Startup")
                        }
                    }
                    // 托盘设置：更新间隔
                    "15" | "30" | "60" | "300" | "600" | "1800" => {
                        config.update_config_event.store(true, Ordering::Release);
                        // 对应ID的子菜单若已勾选，则更新结构体中的配置及配置文件的内容，其余子菜单设置为未勾选
                        tray_check_menus
                            .iter()
                            .filter(|item| {
                                ["15", "30", "60", "300", "600", "1800"]
                                    .contains(&item.id().as_ref())
                            })
                            .for_each(|item| {
                                let id = item.id().as_ref();
                                if id == menu_event_id && item.is_checked() {
                                    let update_interval = id
                                        .parse::<u64>()
                                        .expect("Failed to id parse to update interval(u64)");
                                    config
                                        .tray_config
                                        .update_interval
                                        .store(update_interval, Ordering::Relaxed);
                                    let _ = config
                                        .write_tray_config("update_interval", id)
                                        .inspect_err(|e| {
                                            notify("BlueGauge", &format!("{e}"), config.get_mute())
                                        });
                                } else {
                                    item.set_checked(false)
                                }
                            });
                    }
                    // 通知设置：低电量
                    "0.01" | "0.05" | "0.1" | "0.15" | "0.2" | "0.25" => {
                        tray_check_menus
                            .iter()
                            .filter(|item| {
                                ["0.01", "0.05", "0.1", "0.15", "0.2", "0.25"]
                                    .contains(&item.id().as_ref())
                            })
                            .for_each(|item| {
                                let id = item.id().as_ref();
                                if id == menu_event_id && item.is_checked() {
                                    let low_battery_f64 = id
                                        .parse::<f64>()
                                        .expect("Failed to id parse to low battery(f64)");
                                    let low_battery_u8 = (low_battery_f64 * 100.0).round() as u8;
                                    config
                                        .notify_options
                                        .low_battery
                                        .store(low_battery_u8, Ordering::Relaxed);
                                    let _ = config
                                        .write_notify_options(
                                            "low_battery",
                                            &low_battery_u8.to_string(),
                                        )
                                        .inspect_err(|e| {
                                            notify("BlueGauge", &format!("{e}"), config.get_mute())
                                        });
                                } else {
                                    item.set_checked(false)
                                }
                            });
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
                                let _ = config
                                    .write_notify_options(menu_event_id, "true")
                                    .inspect_err(|e| {
                                        notify("BlueGauge", &format!("{e}"), config.get_mute())
                                    });
                            } else {
                                config.notify_options.update(menu_event_id, false);
                                let _ = config
                                    .write_notify_options(menu_event_id, "false")
                                    .inspect_err(|e| {
                                        notify("BlueGauge", &format!("{e}"), config.get_mute())
                                    });
                            }
                        }
                    }
                    // 托盘设置：提示内容设置
                    "show_disconnected" | "truncate_name" | "prefix_battery" => {
                        config.update_config_event.store(true, Ordering::Release);
                        if let Some(item) = tray_check_menus
                            .iter()
                            .find(|item| item.id().as_ref() == menu_event_id)
                        {
                            if item.is_checked() {
                                config.tray_config.update(menu_event_id, true);
                                let _ =
                                    config.write_tray_config(menu_event_id, "true").inspect_err(
                                        |e| notify("BlueGauge", &format!("{e}"), config.get_mute()),
                                    );
                            } else {
                                config.tray_config.update(menu_event_id, false);
                                let _ = config
                                    .write_tray_config(menu_event_id, "false")
                                    .inspect_err(|e| {
                                        notify("BlueGauge", &format!("{e}"), config.get_mute())
                                    });
                            }
                        }
                    }
                    _ => (),
                }
            }
            UserEvent::UpdateTray => {
                let bluetooth_devices = match find_bluetooth_devices() {
                    Ok(d) => d,
                    Err(e) => {
                        println!("Failed to find bluetooth devices - {e}");
                        return;
                    }
                };
                let new_bt_info = match get_bluetooth_info(bluetooth_devices.0, bluetooth_devices.1)
                {
                    Ok(i) => i,
                    Err(e) => {
                        println!("Failed to get bluetooth devices info - {e}");
                        return;
                    }
                };

                if let Some(e) = compare_bt_info_to_send_notifications(
                    &self.config.lock().unwrap(),
                    Arc::clone(&self.low_battery_devices),
                    Arc::clone(&self.bluetooth_info),
                    new_bt_info,
                ) {
                    e.expect("Failed to compare bluetooth info");
                } else {
                    // 如果此时配置更新有，则不返回Return（配置更新，继续执行后续更新）
                    // 避免蓝牙信息无更新，配置更新，导致托盘提示的设置无效化
                    let config = self.config.lock().unwrap();
                    if !config.update_config_event.swap(false, Ordering::Acquire) {
                        return;
                    }
                }

                let config = self.config.lock().unwrap();

                let (tray_menu, new_tray_check_menus, tooltip, _) =
                    create_menu(&config).expect("Failed to create tray menu");

                if let Some(tray) = &self.tray.lock().unwrap().as_mut() {
                    tray.set_menu(Some(Box::new(tray_menu)));
                    tray.set_tooltip(Some(tooltip.join("\n")))
                        .expect("Failed to update tray tooltip");
                }

                if let Some(tray_check_menus) = self.tray_check_menus.lock().unwrap().as_mut() {
                    *tray_check_menus = new_tray_check_menus;
                }
            }
            UserEvent::UpdateTrayIcon(icon) => {
                if let Some(tray) = &self.tray.lock().unwrap().as_mut() {
                    tray.set_icon(Some(icon))
                        .expect("Failed to update tray icon image")
                }
            }
        }
    }
}
