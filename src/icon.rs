use std::{collections::HashSet, path::Path};

use anyhow::{Context, Result, anyhow};
// use font_kit::{family_name::FamilyName, properties::Properties, source::SystemSource};
// use raqote::{DrawOptions, DrawTarget, SolidSource, Source};
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
        TrayIconSource::BatteryDefault(ref id) | TrayIconSource::BatteryCustom(ref id) => {
            let use_custom_font = matches!(tray_icon_source, TrayIconSource::BatteryCustom(_));

            bluetooth_devices_info
                .iter()
                .find(|i| i.id == *id)
                .map_or(get_icon_from_font(250, use_custom_font), |i| {
                    get_icon_from_font(i.battery, use_custom_font)
                })
        }
    }
}

fn get_icon_from_font(battery_level: u8, use_custom_font: bool) -> Result<Icon> {
    if battery_level == 250 || !use_custom_font {
        let name = format!("{battery_level}_{}", get_system_theme().get_theme_name());
        let icon_data =
            get_image_data(&name).ok_or(anyhow!("Failed to get {battery_level}.png"))?;
        return load_icon(icon_data);

        // let (icon_rgba, icon_width, icon_height) = render_battery_icon(battery_level)?;
        // return Icon::from_rgba(icon_rgba, icon_width, icon_height)
        //     .map_err(|e| anyhow!("Failed to get Icon - {e}"));
    }

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

// fn render_battery_icon(battery_level: u8) -> Result<(Vec<u8>, u32, u32)> {
//     let text = if battery_level == 250 {
//         String::from("X")
//     } else {
//         battery_level.to_string()
//     };
//     let font_color = get_system_theme().get_font_color();

//     // 图标尺寸（可以适配系统托盘）
//     let width = 32;
//     let height = 32;

//     // 创建绘图目标
//     let mut dt = DrawTarget::new(width, height);

//     // 找系统字体（例如 Segoe UI）
//     let font = SystemSource::new()
//         .select_best_match(
//             &[FamilyName::Title("Segoe UI".into()), FamilyName::Monospace],
//             &Properties::new(),
//         )?
//         .load()?;

//     // 设置字体大小
//     let font_size = 20.0;

//     // 渲染文本
//     dt.draw_text(
//         &font,
//         font_size,
//         &text,
//         raqote::Point::new(9.0, 24.0), // 坐标位置
//         &Source::Solid(font_color),
//         &DrawOptions::new(),
//     );

//     // 获取 RGBA 数据
//     let data = dt.get_data_u8().to_vec();

//     Ok((data, width as u32, height as u32))
// }

#[derive(Debug, Clone, Copy, PartialEq)]
enum SystemTheme {
    Light,
    Dark,
}

impl SystemTheme {
    // fn get_font_color(&self) -> SolidSource {
    //     match self {
    //         SystemTheme::Dark => SolidSource {
    //             r: 255,
    //             g: 255,
    //             b: 255,
    //             a: 255,
    //         },
    //         SystemTheme::Light => SolidSource {
    //             r: 0,
    //             g: 0,
    //             b: 0,
    //             a: 255,
    //         },
    //     }
    // }

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
