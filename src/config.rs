use std::path::{Path, PathBuf};
use std::env;

use anyhow::{Result, Context, anyhow};
use ini::Ini;
use glob::glob;

#[derive(Clone)]
pub struct Config {
    pub update_interval: u64,
    pub show_disconnected_devices: bool,
    pub truncate_bluetooth_name: bool,
    pub battery_prefix_name: bool,
    pub icon: Option<ShowIcon>,
    pub notify_mute: bool,
    pub notify_low_battery: Option<u8>,
    pub notify_reconnection: bool,
    pub notify_disconnection: bool,
    pub notify_added_devices: bool,
    pub notify_remove_devices: bool,
}

#[derive(Clone)]
pub enum ShowIcon {
    Logo(PathBuf), // logo.png
    Font(PathBuf), // Font(*.ttf)
    Png(Vec<(PathBuf, u8)>),
    None,
}

pub fn ini() -> Result<(Config, PathBuf)> {
    let exe_path = env::current_exe().context("Failed to get BlueGauge.exe Path")?;
    let exe_dir = exe_path.parent().ok_or(anyhow!("Failed to get BlueGauge.exe parent directory"))?;
    let ini_path = exe_dir.join("config.ini");
    if ini_path.exists() {
        read_ini(exe_dir, ini_path)
    } else {
        create_new_ini(ini_path)
    }
}

fn create_new_ini(ini_path: PathBuf) -> Result<(Config, PathBuf)> {
    let mut ini = Ini::new();

    ini.with_section(Some("Settings"))
        .set("update_interval", "30") // 默认30（单位秒）
        .set("icon", "none") // Value: none、logo、ttf、battery_png（若为图标exe同一目录中存放*.png任一数量的照片，*的范围为0~100，要求每组照片宽高一致）
        .set("show_disconnected_devices", "false")
        .set("truncate_bluetooth_name", "false")
        .set("battery_prefix_name", "false");

    ini.with_section(Some("Notifications"))
        .set("notify_low_battery", "none") // Value：none、number（0~100，单位百分比）
        .set("notify_reconnection", "false")
        .set("notify_disconnection", "false")
        .set("notify_added_devices", "false")
        .set("notify_remove_devices", "flase")
        .set("notify_mute", "false");

    ini.write_to_file(&ini_path).context("Failed to create config.ini")?;

    let config = Config {
        update_interval: 30,
        icon: None,
        show_disconnected_devices: false,
        truncate_bluetooth_name: false,
        battery_prefix_name: false,
        notify_low_battery: None,
        notify_reconnection: false,
        notify_disconnection: false,
        notify_added_devices: false,
        notify_remove_devices: false,
        notify_mute: false,
    };

    Ok((config, ini_path))
}

fn read_ini(exe_dir: &Path, ini_path: PathBuf) -> Result<(Config, PathBuf)> {
    let ini = Ini::load_from_file(&ini_path)
        .context("Failed to load config.ini in BlueGauge.exe directory")?;
    let setting_section = ini.section(Some("Settings"))
        .context("Failed to get 'Settings' Section")?;
    let notifications_section = ini.section(Some("Notifications"))
        .context("Failed to get 'Notifications' Section")?;

    let update_interval = setting_section
        .get("update_interval")
        .filter(|v| !v.trim().is_empty())
        .and_then(|v| v.trim().parse::<u64>().ok())
        .unwrap_or(30);

    let show_disconnected_devices = setting_section.get("show_disconnected_devices")
        .map_or(false, |v| v.trim().to_lowercase() == "true");

    let truncate_bluetooth_name = setting_section.get("truncate_bluetooth_name")
        .map_or(false, |v| v.trim().to_lowercase() == "true");

    let battery_prefix_name = setting_section.get("battery_prefix_name")
        .map_or(false, |v| v.trim().to_lowercase() == "true");

    let icon = setting_section.get("icon").map(|v| {
        match v.trim().to_lowercase().as_str() {
            "logo" => {
                let logo_path = exe_dir.join("logo.png");
                if logo_path.is_file() {
                    ShowIcon::Logo(exe_dir.join("logo.png"))
                } else {
                    ShowIcon::None
                }
            },
            "font" => {
                let font_path = exe_dir.join("font.ttf");
                if font_path.is_file() {
                    ShowIcon::Font(exe_dir.join("font.ttf"))
                } else {
                    ShowIcon::None
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
                    ShowIcon::None
                } else {
                    ShowIcon::Png(battery_indicator_images)
                }
            },
            _ => ShowIcon::None
        }
    });

    let notify_low_battery = notifications_section
        .get("notify_low_battery")
        .filter(|v| !v.trim().is_empty() && v.trim().to_lowercase() != "none")
        .and_then(|v| v.trim().parse::<u8>().ok())
        .filter(|&battery| battery <= 100);

    let notify_reconnection = notifications_section.get("notify_reconnection")
        .map_or(false, |v| v.trim().to_lowercase() == "true");
    let notify_disconnection = notifications_section.get("notify_disconnection")
        .map_or(false, |v| v.trim().to_lowercase() == "true");
    let notify_added_devices = notifications_section.get("notify_added_devices")
        .map_or(false, |v| v.trim().to_lowercase() == "true");
    let notify_remove_devices = notifications_section.get("notify_remove_devices")
        .map_or(false, |v| v.trim().to_lowercase() == "true");
    let notify_mute = notifications_section.get("notify_mute")
        .map_or(false, |v| v.trim().to_lowercase() == "true");

    let config = Config {
        update_interval,
        icon,
        show_disconnected_devices,
        truncate_bluetooth_name,
        battery_prefix_name,
        notify_low_battery,
        notify_reconnection,
        notify_disconnection,
        notify_added_devices,
        notify_remove_devices,
        notify_mute
    };

    Ok((config, ini_path))
}

pub fn write_ini_notifications(ini_path: &Path, key: &str, value: String) {
    let mut ini = Ini::load_from_file(ini_path).expect("Failed to load config.ini in BlueGauge.exe directory");
    ini.set_to(Some("Notifications"), key.to_owned(), value);
    ini.write_to_file(ini_path).expect("Failed to write INI file");
}

pub fn write_ini_settings(ini_path: &Path, key: &str, value: String) {
    let mut ini = Ini::load_from_file(ini_path).expect("Failed to load config.ini in BlueGauge.exe directory");
    ini.set_to(Some("Settings"), key.to_owned(), value);
    ini.write_to_file(ini_path).expect("Failed to write INI file");
}