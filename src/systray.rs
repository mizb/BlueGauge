use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use image;
use tao::{
    event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy},
    platform::run_return::EventLoopExtRunReturn,
};
use tray_icon::{
    menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem},
    TrayIcon, TrayIconBuilder, // TrayIconEvent,
};
use anyhow::{Result, Context};

use crate::bluetooth::{find_bluetooth_devices, get_bluetooth_info, BluetoothInfo};

const ICON_DATA: &[u8] = include_bytes!("../resources/logo.ico");

pub async fn show_systray() -> Result<()> {
    loop_systray().await
}

async fn loop_systray() -> Result<()> {
    let bluetooth_devices = find_bluetooth_devices()
        .await
        .context("Failed to find bluetooth devices")?;
    let bluetooth_devices_info = get_bluetooth_info(bluetooth_devices.0, bluetooth_devices.1)
        .await
        .context("Failed to get bluetooth devices info")?;

    let (tooltip, menu) = convert_tray_info(bluetooth_devices_info);
    let tooltip = Arc::new(Mutex::new(tooltip));
    let menu = Arc::new(Mutex::new(menu));
    let menu_separator = PredefinedMenuItem::separator();
    let menu_quit = MenuItem::new("Quit", true, None);

    let mut tray_icon = TrayIconBuilder::new()
        .with_menu_on_left_click(true)
        .with_icon(load_icon()?)
        .build()
        .context("Failed to build tray")?;

    let mut event_loop = EventLoopBuilder::new().build();
    thread_update_info(
        Arc::clone(&tooltip),
        Arc::clone(&menu),
        event_loop.create_proxy(),
    ).await?;

    let menu_channel = MenuEvent::receiver();
    // let tray_channel = TrayIconEvent::receiver();

    let return_code = event_loop.run_return(|event, _, control_flow| {
        *control_flow =
            ControlFlow::WaitUntil(std::time::Instant::now() + Duration::from_millis(100));

        match event {
            tao::event::Event::NewEvents(tao::event::StartCause::Init) => {
                tray_icon = update_tray(
                    tray_icon.clone(),
                    tooltip.lock().unwrap(),
                    menu.lock().unwrap(),
                    &menu_separator,
                    &menu_quit,
                );
            }
            tao::event::Event::UserEvent(()) => {
                if let (Ok(tooltip_lock), Ok(items_lock)) =
                    (tooltip.try_lock(), menu.try_lock())
                {
                    tray_icon = update_tray(
                        tray_icon.clone(),
                        tooltip_lock,
                        items_lock,
                        &menu_separator,
                        &menu_quit,
                    );
                }
            }
            _ => (),
        };

        // if let Ok(_tary_event) = tray_channel.try_recv() {
        //     println!("Will block updates");
        // }

        if let Ok(menu_event) = menu_channel.try_recv() {
            if menu_event.id == menu_quit.id() {
                println!("process exist");
                *control_flow = ControlFlow::Exit;
            };
        };
    });

    if return_code != 0 {
        std::process::exit(return_code);
    };

    Ok(())
}

fn load_icon() -> Result<tray_icon::Icon> {
    let (icon_rgba, icon_width, icon_height) = {
        let image = image::load_from_memory(ICON_DATA)
            .context("Failed to open icon path")?
            .into_rgba8();
        let (width, height) = image.dimensions();
        let rgba = image.into_raw();
        (rgba, width, height)
    };
    tray_icon::Icon::from_rgba(icon_rgba, icon_width, icon_height).context("Failed to crate the logo")
}

async fn thread_update_info(
    tray_tooltip_clone: Arc<Mutex<Vec<String>>>,
    menu_items_clone: Arc<Mutex<Vec<String>>>,
    event_loop_proxy: EventLoopProxy<()>,
) -> windows::core::Result<()> {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(30)).await;
            let bluetooth_devices = find_bluetooth_devices().await.unwrap();
            let bluetooth_devices_info =
                get_bluetooth_info(bluetooth_devices.0, bluetooth_devices.1).await.unwrap();
            let (tooltip, items) = convert_tray_info(bluetooth_devices_info);
            match (tray_tooltip_clone.try_lock(), menu_items_clone.try_lock()) {
                (Ok(mut tray_tooltip), Ok(mut menu_items)) => {
                    *tray_tooltip = tooltip;
                    *menu_items = items;
                    event_loop_proxy.send_event(()).ok();
                }
                _ => println!("thread: Failed lock attempt"),
            };
        }
    });

    Ok(())
}

fn update_tray(
    tray_icon: TrayIcon,
    tray_tooltip_lock: MutexGuard<Vec<String>>,
    menu_items_lock: MutexGuard<Vec<String>>,
    menu_separator: &PredefinedMenuItem,
    menu_quit: &MenuItem,
) -> TrayIcon {
    let tray_menu = Menu::new();
    tray_menu.append(menu_separator).unwrap();
    tray_menu.append(menu_quit).unwrap();
    menu_items_lock.iter().for_each(|text| {
        let item = MenuItem::new(text, true, None);
        tray_menu.prepend(&item).unwrap();
    });

    tray_icon
        .set_tooltip(Some(tray_tooltip_lock.join("\n")))
        .unwrap();
    tray_icon.set_menu(Some(Box::new(tray_menu)));

    tray_icon
}

fn convert_tray_info(bluetooth_devices_info: Vec<BluetoothInfo>) -> (Vec<String>, Vec<String>) {
    let mut tray_tooltip_result = Vec::new();
    let mut menu_items_result = Vec::new();
    for blue_info in bluetooth_devices_info {
        let name = blue_info.name;
        let battery = blue_info.battery;
        match blue_info.status {
            true => {
                let battery = if battery < 10 {
                    format!("  {battery}")
                } else {
                    battery.to_string()
                };
                tray_tooltip_result
                    .insert(0, format!("🟢 {}% - {}", battery, name));
                menu_items_result.push(format!("{}% - {}", battery, name));
            }
            false => {
                let battery = if battery < 10 {
                    format!("  {battery}")
                } else {
                    battery.to_string()
                };
                tray_tooltip_result.push(format!("🔴 {}% - {}", battery, name));
                menu_items_result.insert(
                    0,
                    format!("{}% - {}", battery, name),
                );
            }
        }
    }
    (tray_tooltip_result, menu_items_result)
}
