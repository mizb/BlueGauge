#![allow(non_snake_case)]
#![cfg(target_os = "windows")]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod bluetooth;
mod config;
mod icon;
mod language;
mod menu_handlers;
mod notify;
mod startup;
mod tray;

use crate::bluetooth::info::{
    BluetoothInfo, compare_bt_info_to_send_notifications, find_bluetooth_devices,
    get_bluetooth_info,
};
use crate::bluetooth::listen::{listen_bluetooth_device_info, listen_bluetooth_devices_info};
use crate::config::*;
use crate::icon::load_battery_icon;
use crate::menu_handlers::MenuHandlers;
use crate::notify::app_notify;
use crate::tray::{convert_tray_info, create_menu, create_tray};

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

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
    std::panic::set_hook(Box::new(|info| {
        app_notify(format!("⚠️ Panic: {info}"));
    }));

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
        let config = Config::open().expect("Failed to open config");

        let bluetooth_devices = find_bluetooth_devices().expect("Failed to find bluetooth devices");
        let bluetooth_devices_info =
            get_bluetooth_info(bluetooth_devices).expect("Failed to get bluetooth devices info");

        let (tray, tray_check_menus) =
            create_tray(&config, &bluetooth_devices_info).expect("Failed to create tray");

        Self {
            bluetooth_info: Arc::new(Mutex::new(bluetooth_devices_info)),
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
    UpdateTray(/* Force Update */ bool), // bool: Force Update
    UpdateTrayForBluetooth(BluetoothInfo),
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

        if let Some(bluetooth_device_address) = config
            .tray_options
            .tray_icon_source
            .lock()
            .unwrap()
            .get_id()
        {
            if let Some(bluetooth_info) = self
                .bluetooth_info
                .lock()
                .unwrap()
                .iter()
                .find(|i| i.address == bluetooth_device_address)
            {
                if let Err(e) =  listen_bluetooth_device_info(Some(bluetooth_info), true, Some(proxy.clone())) {
                    println!("Failed to listen {}: {e}", bluetooth_info.name)
                }
            }
        };

        listen_bluetooth_devices_info(config, proxy);
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
                    .expect("Tray check menus not initialized");

                let menu_event_id = event.id().as_ref();
                match menu_event_id {
                    "quit" => MenuHandlers::qpp_quit(event_loop),
                    "force_update" => MenuHandlers::force_update(&config),
                    "startup" => MenuHandlers::startup(tray_check_menus),
                    "open_config" => MenuHandlers::open_config(),
                    "set_icon_connect_color" => MenuHandlers::set_icon_connect_color(
                        &config,
                        menu_event_id,
                        tray_check_menus,
                    ),
                    // 托盘设置：更新间隔
                    "15" | "30" | "60" | "300" | "600" | "1800" => {
                        MenuHandlers::set_update_interval(&config, menu_event_id, tray_check_menus);
                    }
                    // 通知设置：低电量
                    "0.01" | "0.05" | "0.1" | "0.15" | "0.2" | "0.25" => {
                        MenuHandlers::set_notify_low_battery(
                            &config,
                            menu_event_id,
                            tray_check_menus,
                        );
                    }
                    // 通知设置：静音/断开连接/重新连接/添加/删除
                    "mute" | "disconnection" | "reconnection" | "added" | "removed" => {
                        MenuHandlers::set_notify_device_change(
                            &config,
                            menu_event_id,
                            tray_check_menus,
                        );
                    }
                    // 托盘设置：提示内容设置
                    "show_disconnected" | "truncate_name" | "prefix_battery" => {
                        MenuHandlers::set_tray_tooltip(&config, menu_event_id, tray_check_menus);
                    }
                    _ => {
                        let proxy = self.event_loop_proxy.clone().expect("Failed to get proxy");
                        MenuHandlers::set_tray_icon_source(
                            self.bluetooth_info.lock().unwrap().clone(),
                            &config,
                            menu_event_id,
                            proxy,
                            tray_check_menus,
                        );
                    }
                }
            }
            UserEvent::UpdateTray(need_force_update) => {
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
                    e.expect("Failed to compare bluetooth info");
                } else {
                    // 避免菜单事件或配置更新后，因蓝牙信息无变化而不执行后续更新代码
                    if !need_force_update {
                        return;
                    }
                }

                let (tray_menu, new_tray_check_menus) = match create_menu(&config, &new_bt_info) {
                    Ok(menu) => menu,
                    Err(e) => {
                        app_notify(format!("Failed to create tray  menu - {e}"));
                        return;
                    }
                };

                if let Some(tray) = &self.tray.lock().unwrap().as_mut() {
                    let icon = load_battery_icon(&config, &new_bt_info)
                        .expect("Failed to load battery icon");
                    let bluetooth_tooltip_info = convert_tray_info(&new_bt_info, &config);
                    tray.set_menu(Some(Box::new(tray_menu)));
                    tray.set_tooltip(Some(bluetooth_tooltip_info.join("\n")))
                        .expect("Failed to update tray tooltip");
                    tray.set_icon(Some(icon)).expect("Failed to set tray icon");
                }

                if let Some(tray_check_menus) = self.tray_check_menus.lock().unwrap().as_mut() {
                    *tray_check_menus = new_tray_check_menus;
                }
            }
            UserEvent::UpdateTrayForBluetooth(bluetooth_info) => {
                println!(
                    "Need to update the info immediately: {}",
                    bluetooth_info.name
                );
                let current_bt_info = {
                    let mut original_bt_info = self.bluetooth_info.lock().unwrap();
                    original_bt_info.retain(|i| i.address != bluetooth_info.address);
                    original_bt_info.insert(bluetooth_info);
                    original_bt_info.clone()
                };

                let config = Arc::clone(&self.config);

                let (tray_menu, new_tray_check_menus) = match create_menu(&config, &current_bt_info)
                {
                    Ok(menu) => menu,
                    Err(e) => {
                        app_notify(format!("Failed to create tray menu - {e}"));
                        return;
                    }
                };

                if let Some(tray) = &self.tray.lock().unwrap().as_mut() {
                    let icon = load_battery_icon(&config, &current_bt_info)
                        .expect("Failed to load battery icon");
                    let bluetooth_tooltip_info = convert_tray_info(&current_bt_info, &config);
                    tray.set_menu(Some(Box::new(tray_menu)));
                    tray.set_tooltip(Some(bluetooth_tooltip_info.join("\n")))
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
