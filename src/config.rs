use std::path::{Path, PathBuf};
use std::env;

use anyhow::{Result, Context, anyhow};
use ini::Ini;
use glob::glob;

pub struct Config {
    pub update_interval: u64,
    pub icon: Option<TrayIcon>,
    pub notify_low_battery: Option<u8>,
    pub notify_reconnection: bool,
    pub notify_disconnection: bool,
    pub notify_new_devices: bool,
    pub notify_remove_devices: bool,
    pub notify_mute: bool,
}

pub enum TrayIcon {
    Logo(PathBuf), // logo.png
    Font(PathBuf), // Font(*.ttf)
    Png(Vec<(PathBuf, u8)>),
    None,
}

pub fn ini() -> Result<Config> {
    let exe_path = env::current_exe().context("Failed to get BlueGauge.exe Path")?;
    let exe_dir = exe_path.parent().ok_or(anyhow!("Failed to get BlueGauge.exe parent directory"))?;
    let ini_path = exe_dir.join("config.ini");
    if ini_path.exists() {
        read_ini(exe_dir)
    } else {
        create_ini()
    }
}

fn create_ini() -> Result<Config> {
    let mut ini = Ini::new();

    ini.with_section(Some("Setting"))
        .set("update_interval", "30") // Value: 数字（范围：0~100，单位为秒）
        .set("icon", ""); // Value: logo、ttf、battery_png（若为图标exe同一目录中存放*.png任一数量的照片，*的范围为0~100，要求每组照片宽高一致）

    ini.with_section(Some("Notifications"))
        .set("notify_low_battery", "") // 15（默认单位百分比）
        .set("notify_reconnection", "")
        .set("notify_disconnection", "")
        .set("notify_new_devices", "")
        .set("notify_remove_devices", "")
        .set("notify_mute", "");

    ini.write_to_file("config.ini").context("Failed to create config.ini")?;

    let config = Config {
        update_interval: 30,
        icon: None,
        notify_low_battery: None,
        notify_reconnection: false,
        notify_disconnection: false,
        notify_new_devices: false,
        notify_remove_devices: false,
        notify_mute: false,
    };

    Ok(config)
}

fn read_ini(exe_dir: &Path) -> Result<Config> {
    let ini = Ini::load_from_file("config.ini").context("Failed to load config.ini")?;
    let setting_section = ini.section(Some("Setting")).context("Failed to get 'Setting' Section")?;
    let notifications_section = ini.section(Some("Notifications")).context("Failed to get 'Notifications' Section")?;

    let update_interval = match setting_section.get("update_interval") {
        Some(v) => {
            if v.is_empty() {
                30
            } else {
                v.trim().parse::<u64>().context("'update_interval' is not a number.")?
            }
        },
        None => 30,
    };

    let icon = setting_section.get("icon").map(|v| {
        match v.trim().to_lowercase().as_str() {
            "logo" => {
                let logo_path = exe_dir.join("logo.png");
                if logo_path.is_file() {
                    TrayIcon::Logo(exe_dir.join("logo.png"))
                } else {
                    TrayIcon::None
                }
            },
            "font" => {
                let font_path = exe_dir.join("font.ttf");
                if font_path.is_file() {
                    TrayIcon::Font(exe_dir.join("font.ttf"))
                } else {
                    TrayIcon::None
                }
            },
            "battery_png" => {
                let battery_indicator_images = glob("*.png")
                    .unwrap()
                    .filter_map(Result::ok)
                    .filter_map(|path| {
                        path.file_stem()
                            .and_then(|name| name.to_string_lossy().trim().parse::<u8>().ok())
                            .filter(|&battery| battery <= 100)
                            .map(|battery| (path, battery))
                    })
                    .collect::<Vec<_>>();
                if battery_indicator_images.is_empty() {
                    TrayIcon::None
                } else {
                    TrayIcon::Png(battery_indicator_images)
                }
            },
            _ => TrayIcon::None
        }
    });

    let notify_low_battery = match notifications_section.get("notify_low_battery") {
        Some(v) => {
            if v.trim().is_empty() {
                None
            } else {
                if let Ok(battery) = v.trim().parse::<u8>() {
                    if battery <= 100 {
                        Some(battery)
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
        }
        None => None
    };

    let notify_reconnection = notifications_section.get("notify_reconnection")
        .map_or(false, |v| v.trim().to_lowercase() == "true");
    let notify_disconnection = notifications_section.get("notify_disconnection")
        .map_or(false, |v| v.trim().to_lowercase() == "true");
    let notify_new_devices = notifications_section.get("notify_new_devices")
        .map_or(false, |v| v.trim().to_lowercase() == "true");
    let notify_remove_devices = notifications_section.get("notify_remove_devices")
        .map_or(false, |v| v.trim().to_lowercase() == "true");
    let notify_mute = notifications_section.get("notify_mute")
        .map_or(false, |v| v.trim().to_lowercase() == "true");

    let config = Config {
        update_interval,
        icon,
        notify_low_battery,
        notify_reconnection,
        notify_disconnection,
        notify_new_devices,
        notify_remove_devices,
        notify_mute
    };

    Ok(config)
}