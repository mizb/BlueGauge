use std::sync::{Arc, Mutex};
use std::time::Duration;

use image;
use tao::{
    event_loop::{ControlFlow, EventLoopBuilder},
    platform::run_return::EventLoopExtRunReturn,
};
use tray_icon::{
    menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem},
    TrayIconBuilder, // TrayIconEvent,
};
use anyhow::{Result, Context, anyhow};

use crate::bluetooth::{find_bluetooth_devices, get_bluetooth_info, BluetoothInfo};

const ICON_DATA: &[u8] = include_bytes!("../resources/logo.ico");

// enum TrayBatteryIcon {
//     Font,
//     Png
// }

pub async fn show_systray() -> Result<()> {
    loop_systray().await
}

async fn loop_systray() -> Result<()> {
    let (tooltip, menu) = get_bluetooth_tray_info(true).await?;

    let tooltip = Arc::new(Mutex::new(tooltip));
    let tootip_clone = Arc::clone(&tooltip);

    let menu_separator = PredefinedMenuItem::separator();
    let menu_quit = MenuItem::new("Quit", true, None);
    let tray_menu = Menu::new();
    menu.iter().for_each(|text| {
        let item = MenuItem::new(text, true, None);
        tray_menu.append(&item).unwrap();
    });
    tray_menu.append(&menu_separator).context("Failed to apped 'Separator' to Tray Menu")?;
    tray_menu.append(&menu_quit).context("Failed to apped 'Quit' to Tray Menu")?;

    let tray_icon = TrayIconBuilder::new()
        .with_menu_on_left_click(true)
        .with_icon(load_icon()?)
        .with_tooltip(tooltip.lock().unwrap().join("\n"))
        .with_menu(Box::new(tray_menu))
        .build()
        .context("Failed to build tray")?;

    let mut event_loop = EventLoopBuilder::new().build();
    let event_loop_proxy = event_loop.create_proxy();

    tokio::task::spawn(async move {
        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;
            match get_bluetooth_tray_info(false).await {
                Ok((t, _ )) => {
                    if let Ok(mut tooltip) = tootip_clone.try_lock() {
                        *tooltip = t;
                        event_loop_proxy.send_event(()).ok();
                    } else {
                        println!("Failed to acquire tooltip lock on task")
                    }
                },
                Err(e) => println!("{e}")
            };
        }
    });

    let menu_channel = MenuEvent::receiver();
    // let tray_channel = TrayIconEvent::receiver();

    let return_code = event_loop.run_return(|event, _, control_flow| {
        *control_flow =
            ControlFlow::WaitUntil(std::time::Instant::now() + Duration::from_millis(100));

        match event {
            tao::event::Event::UserEvent(()) => {
                if let Ok(t) = tooltip.try_lock() {
                    tray_icon.set_tooltip(Some(t.join("\n"))).expect("Failed to update tray tooltip");
                };
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

async fn get_bluetooth_tray_info(need_menu: bool) -> Result<(Vec<String>, Vec<String>)> {
    let bluetooth_devices = find_bluetooth_devices()
        .await
        .map_err(|e| anyhow!("Failed to find bluetooth devices - {e}"))?;
    let bluetooth_devices_info = get_bluetooth_info(bluetooth_devices.0, bluetooth_devices.1)
        .await
        .map_err(|e| anyhow!("Failed to get bluetooth devices info - {e}"))?;
    let tooltip = convert_tray_tooltip(&bluetooth_devices_info);
    let menu = if need_menu {
        convert_tray_menu(&bluetooth_devices_info)
    } else {
        Vec::new()
    };
    Ok((tooltip, menu))
}

fn convert_tray_tooltip(bluetooth_devices_info: &[BluetoothInfo]) -> Vec<String> {
    bluetooth_devices_info.iter().fold(Vec::new(), |mut acc, blue_info| {
        let name = truncate_with_ellipsis(&blue_info.name, 10);
        let battery = blue_info.battery;
        let status_icon = if blue_info.status { "🟢" } else { "🔴" };
        // let status_icon = if blue_info.status { "[●]" } else { "[−]" };
        let info = format!("{status_icon}{battery:3}% - {name}");

        match blue_info.status {
            true => acc.insert(0, info),
            false => acc.push(info)
        }

        acc
    })
}

fn convert_tray_menu(bluetooth_devices_info: &Vec<BluetoothInfo>) -> Vec<String> {
    bluetooth_devices_info.iter().fold(Vec::new(), |mut acc, blue_info| {
        match blue_info.status {
            true => acc.insert(0, blue_info.name.to_owned()),
            false => acc.push(blue_info.name.to_owned())
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