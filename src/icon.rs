use std::{collections::HashSet, path::Path};

use anyhow::{Context, Result, anyhow};
use piet_common::{
    Color, Device, FontFamily, ImageFormat, RenderContext, Text, TextLayout, TextLayoutBuilder,
};
use tray_icon::Icon;
use winreg::{
    RegKey,
    enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE},
};

use crate::{
    bluetooth::BluetoothInfo,
    config::{Config, TrayIconSource},
};

pub const ICON_DATA: &[u8] = include_bytes!("../assets/logo.ico");
const PERSONALIZE_REGISTRY_KEY: &str =
    r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize";
const APPS_USE_LIGHT_THEME_REGISTRY_KEY: &str = "AppsUseLightTheme";

include!(concat!(env!("OUT_DIR"), "/images.rs"));
fn get_image_data(name: &str) -> Option<&'static [u8]> {
    // 使用 phf::Map 的 .get() 方法来查找数据
    // .copied() 将 Option<&&'static [u8]> 转换为 Option<&'static [u8]>
    IMAGES.get(name).copied()
}

pub fn load_icon(icon_date: &[u8]) -> Result<Icon> {
    let (icon_rgba, icon_width, icon_height) = {
        let image = image::load_from_memory(icon_date)
            .with_context(|| "Failed to open icon path")?
            .into_rgba8();
        let (width, height) = image.dimensions();
        let rgba = image.into_raw();
        (rgba, width, height)
    };
    Icon::from_rgba(icon_rgba, icon_width, icon_height).with_context(|| "Failed to crate the logo")
}

pub fn load_battery_icon(
    config: &Config,
    bluetooth_devices_info: &HashSet<BluetoothInfo>,
) -> Result<Icon> {
    let default_icon =
        || load_icon(ICON_DATA).map_err(|e| anyhow!("Failed to load app icon - {e}"));

    let tray_icon_source = {
        let lock = config.tray_config.tray_icon_source.lock().unwrap();
        lock.clone()
    };

    match tray_icon_source {
        TrayIconSource::App => default_icon(),
        TrayIconSource::BatteryDefault { ref id } | TrayIconSource::BatteryCustom { ref id } | TrayIconSource::BatteryFont { ref id, ..} => {
            bluetooth_devices_info
                .iter()
                .find(|i| i.id == *id)
                .map_or(get_icon_from_custom(250), |i| {
                    match tray_icon_source {
                        TrayIconSource::BatteryCustom { .. } => get_icon_from_custom(i.battery),
                        TrayIconSource::BatteryDefault { .. } => get_icon_from_image(i.battery),
                        TrayIconSource::BatteryFont {id: _ , font_name, font_color} => get_icon_from_font(i.battery, &font_name, font_color),
                        _ => get_icon_from_custom(250)
                    }
                })
        }
    }
}

fn get_icon_from_image(battery_level: u8) -> Result<Icon> {
    let image_name = format!("{battery_level}_{}", get_system_theme().get_theme_name());
    let icon_data =
        get_image_data(&image_name).ok_or(anyhow!("Failed to get {battery_level}.png"))?;
    load_icon(icon_data)
}

fn get_icon_from_custom(battery_level: u8) -> Result<Icon> {
    let custom_battery_icon_path = std::env::current_exe()
        .ok()
        .and_then(|exe_path| exe_path.parent().map(Path::to_path_buf))
        .map(|p| p.join(format!("assets\\{battery_level}.png")))
        .and_then(|p| p.is_file().then_some(p))
        .ok_or(anyhow!(
            "Failed to find {battery_level}.png in Bluegauge directory"
        ))?;

    let icon_data = std::fs::read(custom_battery_icon_path)?;

    load_icon(&icon_data)
}

fn get_icon_from_font(battery_level: u8, font_name: &str, color: Option<String>) -> Result<Icon> {
    let (icon_rgba, icon_width, icon_height) = render_battery_icon(battery_level, font_name, color)?;
    Icon::from_rgba(icon_rgba, icon_width, icon_height)
        .map_err(|e| anyhow!("Failed to get Icon - {e}"))
}

fn render_battery_icon(battery_level: u8, font_name: &str, font_color: Option<String>) -> Result<(Vec<u8>, u32, u32)> {
    let indicator = if battery_level == 250 {
        String::from("X")
    } else {
        battery_level.to_string()
    };

    let font_color = font_color.map_or(get_system_theme().get_font_color(), |c| c.parse::<u32>().unwrap());

    let width = 32;
    let height = 32;

    let mut device = Device::new().map_err(|e| anyhow!("Failed to get Device - {e}"))?;

    let mut bitmap_target = device
        .bitmap_target(width, height, 1.0)
        .map_err(|e| anyhow!("Failed to create a new bitmap target. - {e}"))?;

    let mut piet = bitmap_target.render_context();

    // Dynamically calculated font size
    let mut layout;
    let mut font_size = match battery_level {
        100 => 20.0,
        b  if b < 10 => 36.0,
        _ => 32.0,
    };
    let text = piet.text();
    loop {
        layout = text
            .new_text_layout(indicator.clone())
            .font(FontFamily::new_unchecked(font_name), font_size)
            .text_color(Color::from_rgba32_u32(font_color)) // 0xffffff + alpha:00~ff
            .build()
            .map_err(|e| anyhow!("Failed to build text layout - {e}"))?;

        if layout.size().width > width as f64 || layout.size().height > height as f64 {
            break;
        }
        font_size += 2.0;
    }

    let (x, y) = (
        (width as f64 - layout.size().width) / 2.0,
        (height as f64 - layout.size().height) / 2.0,
    );

    piet.draw_text(&layout, (x, y));
    piet.finish().map_err(|e| anyhow!("{e}"))?;
    drop(piet);

    let image_buf = bitmap_target.to_image_buf(ImageFormat::RgbaPremul).unwrap();

    Ok((
        image_buf.raw_pixels().to_vec(),
        image_buf.width() as u32,
        image_buf.height() as u32,
    ))
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum SystemTheme {
    Light,
    Dark,
}

impl SystemTheme {
    fn get_font_color(&self) -> u32 {
        match self {
            SystemTheme::Dark => 0xFFFFFFFF,
            SystemTheme::Light => 0x1F1F1FFF,
        }
    }

    fn get_theme_name(&self) -> &str {
        match self {
            SystemTheme::Dark => "light",
            Self::Light => "dark",
        }
    }
}

fn get_system_theme() -> SystemTheme {
    let personalize_reg_key = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey_with_flags(PERSONALIZE_REGISTRY_KEY, KEY_READ | KEY_WRITE)
        .expect("This program requires Windows 10 14393 or above");

    let theme_reg_value: u32 = personalize_reg_key
        .get_value(APPS_USE_LIGHT_THEME_REGISTRY_KEY)
        .expect("This program requires Windows 10 14393 or above");

    match theme_reg_value {
        0 => SystemTheme::Dark,
        _ => SystemTheme::Light,
    }
}
