use std::collections::HashSet;

use crate::bluetooth::{BluetoothInfo, find_bluetooth_devices, get_bluetooth_info};
use crate::config::Config;
use crate::icon::{ICON_DATA, load_battery_icon, load_icon};
use crate::language::{Language, Localization};
use crate::notify::app_notify;
use crate::startup::get_startup_status;

use anyhow::{Context, Result, anyhow};
use tray_icon::menu::{IsMenuItem, Submenu};
use tray_icon::{
    TrayIcon, TrayIconBuilder,
    menu::{AboutMetadata, CheckMenuItem, Menu, MenuItem, PredefinedMenuItem},
};

type TrayMenuResult = (
    Menu,
    Vec<CheckMenuItem>,
    Vec<String>,            // Tooltip
    HashSet<BluetoothInfo>, // Already Notified Set
);

pub fn create_menu(config: &Config) -> Result<TrayMenuResult> {
    let language = Language::get_system_language();
    let loc = Localization::get(language);

    let mut tray_check_menus: Vec<CheckMenuItem> = Vec::new();

    let tray_menu = Menu::new();

    let menu_separator = PredefinedMenuItem::separator();

    let menu_quit = MenuItem::with_id("quit", loc.quit, true, None);

    let menu_about = PredefinedMenuItem::about(
        Some(loc.about),
        Some(AboutMetadata {
            name: Some("BlueGauge".to_owned()),
            version: Some("0.2.2".to_owned()),
            authors: Some(vec!["iKineticate".to_owned()]),
            website: Some("https://github.com/iKineticate/BlueGauge".to_owned()),
            ..Default::default()
        }),
    );

    let menu_force_update = MenuItem::with_id("force_update", loc.force_update, true, None);

    // 获取蓝牙设备电量并添加至菜单
    let bluetooth_devices =
        find_bluetooth_devices().map_err(|e| anyhow!("Failed to find bluetooth devices - {e}"))?;
    let bluetooth_devices_info = get_bluetooth_info(bluetooth_devices)
        .map_err(|e| anyhow!("Failed to get bluetooth devices info - {e}"))?;

    let bluetooth_tooltip_info = convert_tray_info(&bluetooth_devices_info, config);

    let show_tray_battery_icon_bt_id = config.get_tray_battery_icon_bt_id();
    let bluetooth_check_items: Vec<CheckMenuItem> = bluetooth_devices_info
        .iter()
        .map(|info| {
            CheckMenuItem::with_id(
                &info.id,
                &info.name,
                true,
                show_tray_battery_icon_bt_id
                    .as_deref()
                    .is_some_and(|id| id.eq(&info.id)),
                None,
            )
        })
        .collect();
    let bluetooth_items: Vec<&dyn IsMenuItem> = bluetooth_check_items
        .iter()
        .map(|item| item as &dyn IsMenuItem)
        .collect();
    tray_check_menus.extend(bluetooth_check_items.iter().cloned());

    // 自启动菜单
    let should_startup = get_startup_status()?;
    let menu_startup = &CheckMenuItem::with_id("startup", loc.startup, true, should_startup, None);
    tray_check_menus.push(menu_startup.clone());

    // 更新间隔菜单
    let update_interval = config.get_update_interval();
    let update_interval_items = [
        CheckMenuItem::with_id("15", "15s", true, update_interval == 15, None),
        CheckMenuItem::with_id("30", "30s", true, update_interval == 30, None),
        CheckMenuItem::with_id("60", "1min", true, update_interval == 60, None),
        CheckMenuItem::with_id("300", "5min", true, update_interval == 300, None),
        CheckMenuItem::with_id("600", "10min", true, update_interval == 600, None),
        CheckMenuItem::with_id("1800", "30min", true, update_interval == 1800, None),
    ];
    tray_check_menus.extend(update_interval_items.iter().cloned());
    let update_interval_items: Vec<&dyn IsMenuItem> = update_interval_items
        .iter()
        .map(|item| item as &dyn IsMenuItem)
        .collect();
    let update_interval_submenu = &Submenu::with_id_and_items(
        "update_interval",
        loc.update_interval,
        true,
        &update_interval_items,
    )? as &dyn IsMenuItem;
    // 托盘选项菜单
    let tray_tooltip_items = [
        (
            "show_disconnected",
            loc.show_disconnected,
            config.get_show_disconnected(),
        ),
        (
            "truncate_name",
            loc.truncate_name,
            config.get_truncate_name(),
        ),
        (
            "prefix_battery",
            loc.prefix_battery,
            config.get_prefix_battery(),
        ),
    ];
    let tray_tooltip_check_items: Vec<CheckMenuItem> = tray_tooltip_items
        .into_iter()
        .map(|(id, name, is_checked)| CheckMenuItem::with_id(id, name, true, is_checked, None))
        .collect();
    tray_check_menus.extend(tray_tooltip_check_items.iter().cloned());
    let mut tray_config_check_menus: Vec<&dyn IsMenuItem> = tray_tooltip_check_items
        .iter()
        .map(|item| item as &dyn IsMenuItem)
        .collect();
    tray_config_check_menus.insert(0, update_interval_submenu);
    let tray_config_submenu =
        &Submenu::with_items(loc.tray_config, true, &tray_config_check_menus)?;

    // 低电量通知菜单
    let low_battery = config.get_low_battery();
    let low_battery_items = [
        CheckMenuItem::with_id("0.01", "1%", true, low_battery == 0, None),
        CheckMenuItem::with_id("0.05", "5%", true, low_battery == 5, None),
        CheckMenuItem::with_id("0.1", "10%", true, low_battery == 10, None),
        CheckMenuItem::with_id("0.15", "15%", true, low_battery == 15, None),
        CheckMenuItem::with_id("0.2", "20%", true, low_battery == 20, None),
        CheckMenuItem::with_id("0.25", "25%", true, low_battery == 25, None),
    ];
    tray_check_menus.extend(low_battery_items.iter().cloned());
    let low_battery_items: Vec<&dyn IsMenuItem> = low_battery_items
        .iter()
        .map(|item| item as &dyn IsMenuItem)
        .collect();
    let low_battery_submenu =
        &Submenu::with_items(loc.low_battery, true, &low_battery_items)? as &dyn IsMenuItem;
    // 通知选项菜单
    let notify_options_items = vec![
        ("mute", loc.mute, config.get_mute()),
        (
            "disconnection",
            loc.disconnection,
            config.get_disconnection(),
        ),
        ("reconnection", loc.reconnection, config.get_reconnection()),
        ("added", loc.added, config.get_added()),
        ("removed", loc.removed, config.get_removed()),
    ];
    let notify_options_check_items: Vec<CheckMenuItem> = notify_options_items
        .into_iter()
        .map(|(id, name, is_checked)| CheckMenuItem::with_id(id, name, true, is_checked, None))
        .collect();
    tray_check_menus.extend(notify_options_check_items.iter().cloned());
    let mut notify_options_check_menus: Vec<&dyn IsMenuItem> = notify_options_check_items
        .iter()
        .map(|item| item as &dyn IsMenuItem)
        .collect();
    notify_options_check_menus.insert(0, low_battery_submenu);
    let notify_options_submenu =
        &Submenu::with_items(loc.notify_options, true, &notify_options_check_menus)?;

    // 设置菜单
    let settings_items = &[
        tray_config_submenu as &dyn IsMenuItem,
        notify_options_submenu as &dyn IsMenuItem,
        menu_startup as &dyn IsMenuItem,
    ];
    let menu_setting = Submenu::with_items(loc.settings, true, settings_items)?;

    tray_menu
        .prepend_items(&bluetooth_items)
        .context("Failed to prepend 'Bluetooth Items' to Tray Menu")?;
    tray_menu
        .append(&menu_separator)
        .context("Failed to apped 'Separator' to Tray Menu")?;
    tray_menu
        .append(&menu_setting)
        .context("Failed to apped 'Settings' to Tray Menu")?;
    tray_menu
        .append(&menu_separator)
        .context("Failed to apped 'Separator' to Tray Menu")?;
    tray_menu
        .append(&menu_force_update)
        .context("Failed to apped 'Force Update' to Tray Menu")?;
    tray_menu
        .append(&menu_separator)
        .context("Failed to apped 'Separator' to Tray Menu")?;
    tray_menu
        .append(&menu_about)
        .context("Failed to apped 'About' to Tray Menu")?;
    tray_menu
        .append(&menu_separator)
        .context("Failed to apped 'Separator' to Tray Menu")?;
    tray_menu
        .append(&menu_quit)
        .context("Failed to apped 'Quit' to Tray Menu")?;

    Ok((
        tray_menu,
        tray_check_menus,
        bluetooth_tooltip_info,
        bluetooth_devices_info,
    ))
}

#[rustfmt::skip]
pub fn create_tray(
    config: &Config,
) -> Result<(TrayIcon, Vec<CheckMenuItem>, HashSet<BluetoothInfo>)> {
    let (tray_menu, tray_check_menus, bluetooth_tooltip_info, bluetooth_info) =
        create_menu(config).map_err(|e| anyhow!("Failed to create menu. - {e}"))?;

    let icon = load_battery_icon(config, &bluetooth_info)
        .inspect_err(|e| app_notify(format!("Failed to get battery icon: {e}")))
        .unwrap_or(load_icon(ICON_DATA)?);

    let tray_icon = TrayIconBuilder::new()
        .with_menu_on_left_click(true)
        .with_icon(icon)
        .with_tooltip(bluetooth_tooltip_info.join("\n"))
        .with_menu(Box::new(tray_menu))
        .build()
        .map_err(|e| anyhow!("Failed to build tray - {e}"))?;

    Ok((tray_icon, tray_check_menus, bluetooth_info))
}

/// 返回托盘提示及菜单内容
fn convert_tray_info(
    bluetooth_devices_info: &HashSet<BluetoothInfo>,
    config: &Config,
) -> Vec<String> {
    let should_truncate_name = config.get_truncate_name();
    let should_prefix_battery = config.get_prefix_battery();
    let should_show_disconnected = config.get_show_disconnected();

    let mut tray_tooltip_info: Vec<String> = Vec::new();

    bluetooth_devices_info.iter().for_each(|blue_info| {
        let name = truncate_with_ellipsis(should_truncate_name, &blue_info.name, 10);
        let battery = blue_info.battery;
        let status_icon = if blue_info.status { "🟢" } else { "🔴" };
        let info = if should_prefix_battery {
            format!("{status_icon}{battery:3}% - {name}")
        } else {
            format!("{status_icon}{name} - {battery:3}%")
        };
        match blue_info.status {
            true => tray_tooltip_info.push(info),
            false if should_show_disconnected => tray_tooltip_info.push(info),
            _ => (),
        };
    });
    tray_tooltip_info
}

fn truncate_with_ellipsis(truncate_device_name: bool, name: &str, max_chars: usize) -> String {
    if truncate_device_name && name.chars().count() > max_chars {
        let mut result = name.chars().take(max_chars).collect::<String>();
        result.push_str("...");
        result
    } else {
        name.to_string()
    }
}
