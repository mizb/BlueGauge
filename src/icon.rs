use crate::{
    bluetooth::BluetoothInfo,
    config::{Config, TrayIconSource},
};

use std::collections::HashSet;

use anyhow::{Context, Result, anyhow};
use piet_common::{
    Color, Device, FontFamily, ImageFormat, RenderContext, Text, TextLayout, TextLayoutBuilder,
};
use tray_icon::Icon;
use winreg::{
    RegKey,
    enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WRITE},
};

pub const LOGO_DATA: &[u8] = include_bytes!("../assets/logo.ico");
const UNPAIRED_ICON_DATA: &[u8] = include_bytes!("../assets/unpaired.png");
const PERSONALIZE_REGISTRY_KEY: &str =
    r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize";
const SYSTEM_USES_LIGHT_THEME_REGISTRY_KEY: &str = "SystemUsesLightTheme";

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
        || load_icon(LOGO_DATA).map_err(|e| anyhow!("Failed to load app icon - {e}"));

    let tray_icon_source = {
        let lock = config.tray_config.tray_icon_source.lock().unwrap();
        lock.clone()
    };

    match tray_icon_source {
        TrayIconSource::App => default_icon(),
        TrayIconSource::BatteryCustom { ref id } | TrayIconSource::BatteryFont { ref id, .. } => {
            bluetooth_devices_info.iter().find(|i| i.id == *id).map_or(
                load_icon(UNPAIRED_ICON_DATA),
                |i| match tray_icon_source {
                    TrayIconSource::BatteryCustom { .. } => get_icon_from_custom(i.battery),
                    TrayIconSource::BatteryFont {
                        id: _,
                        font_name,
                        font_color,
                        font_size,
                    } => get_icon_from_font(i.battery, &font_name, font_color, font_size),
                    _ => load_icon(UNPAIRED_ICON_DATA),
                },
            )
        }
    }
}

fn get_icon_from_custom(battery_level: u8) -> Result<Icon> {
    let custom_battery_icon_path = std::env::current_exe()
        .map(|exe_path| exe_path.with_file_name("assets"))
        .and_then(|icon_dir| {
            let default_icon_path = icon_dir.join(format!("{battery_level}.png"));
            if default_icon_path.is_file() {
                return Ok(default_icon_path);
            }
            let theme_icon = match SystemTheme::get() {
                SystemTheme::Light => icon_dir.join(format!("light\\{battery_level}.png")),
                SystemTheme::Dark => icon_dir.join(format!("dark\\{battery_level}.png")),
            };
            if theme_icon.is_file() {
                return Ok(theme_icon);
            }
            Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("Failed to find {battery_level} default/theme PNG in Bluegauge directory"),
            ))
        })?;

    let icon_data = std::fs::read(custom_battery_icon_path)?;

    load_icon(&icon_data)
}

fn get_icon_from_font(
    battery_level: u8,
    font_name: &str,
    font_color: Option<String>,
    font_size: Option<u8>,
) -> Result<Icon> {
    let (icon_rgba, icon_width, icon_height) =
        render_battery_font_icon(battery_level, font_name, font_color, font_size)?;
    Icon::from_rgba(icon_rgba, icon_width, icon_height)
        .map_err(|e| anyhow!("Failed to get Icon - {e}"))
}

fn render_battery_font_icon(
    battery_level: u8,
    font_name: &str,
    font_color: Option<String>, // 格式：#123456、#123456FF
    font_size: Option<u8>,
) -> Result<(Vec<u8>, u32, u32)> {
    let indicator = battery_level.to_string();

    let width = 64;
    let height = 64;
    let font_color = font_color
        .and_then(|c| c.ne("FollowSystemTheme").then_some(c))
        .unwrap_or_else(|| SystemTheme::get().get_font_color());

    let mut device = Device::new().map_err(|e| anyhow!("Failed to get Device - {e}"))?;

    let mut bitmap_target = device
        .bitmap_target(width, height, 1.0)
        .map_err(|e| anyhow!("Failed to create a new bitmap target. - {e}"))?;

    let mut piet = bitmap_target.render_context();

    // Dynamically calculated font size
    let mut layout;
    let text = piet.text();
    if let Some(size) = font_size {
        layout = text
            .new_text_layout(indicator.clone())
            .font(FontFamily::new_unchecked(font_name), size as f64)
            .text_color(Color::from_hex_str(&font_color)?)
            .build()
            .map_err(|e| anyhow!("Failed to build text layout - {e}"))?;
    } else {
        let mut font_size = match battery_level {
            100 => 42.0,
            b if b < 10 => 70.0,
            _ => 64.0,
        };
        loop {
            layout = text
                .new_text_layout(indicator.clone())
                .font(FontFamily::new_unchecked(font_name), font_size)
                .text_color(Color::from_hex_str(&font_color)?)
                .build()
                .map_err(|e| anyhow!("Failed to build text layout - {e}"))?;

            if layout.size().width > width as f64 || layout.size().height > height as f64 {
                break;
            }
            font_size += 2.0;
        }
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
    fn get() -> Self {
        let personalize_reg_key = RegKey::predef(HKEY_CURRENT_USER)
            .open_subkey_with_flags(PERSONALIZE_REGISTRY_KEY, KEY_READ | KEY_WRITE)
            .expect("This program requires Windows 10 14393 or above");

        let theme_reg_value: u32 = personalize_reg_key
            .get_value(SYSTEM_USES_LIGHT_THEME_REGISTRY_KEY)
            .expect("This program requires Windows 10 14393 or above");

        match theme_reg_value {
            0 => SystemTheme::Dark,
            _ => SystemTheme::Light,
        }
    }

    fn get_font_color(&self) -> String {
        match self {
            Self::Dark => "#FFFFFF".to_owned(),
            Self::Light => "#1F1F1F".to_owned(),
        }
    }
}
