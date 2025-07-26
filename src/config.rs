use std::env;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};

use anyhow::{Context, Result, anyhow};
// use glob::glob;
use ini::Ini;

#[derive(Default, Debug)]
pub struct Config {
    pub config_path: PathBuf,
    pub notify_options: NotifyOptions,
    pub tray_config: TrayConfig,
    pub update_config_event: AtomicBool,
}

impl Config {
    pub fn get_update_interval(&self) -> u64 {
        self.tray_config.update_interval.load(Ordering::Acquire)
    }

    pub fn get_prefix_battery(&self) -> bool {
        self.tray_config
            .tooltip_options
            .prefix_battery
            .load(Ordering::Acquire)
    }

    pub fn get_show_disconnected(&self) -> bool {
        self.tray_config
            .tooltip_options
            .show_disconnected
            .load(Ordering::Acquire)
    }

    pub fn get_truncate_name(&self) -> bool {
        self.tray_config
            .tooltip_options
            .truncate_name
            .load(Ordering::Acquire)
    }

    pub fn get_mute(&self) -> bool {
        self.notify_options.mute.load(Ordering::Acquire)
    }

    pub fn get_low_battery(&self) -> u8 {
        self.notify_options.low_battery.load(Ordering::Acquire)
    }

    pub fn get_disconnection(&self) -> bool {
        self.notify_options.disconnection.load(Ordering::Acquire)
    }

    pub fn get_reconnection(&self) -> bool {
        self.notify_options.reconnection.load(Ordering::Acquire)
    }

    pub fn get_added(&self) -> bool {
        self.notify_options.added.load(Ordering::Acquire)
    }

    pub fn get_removed(&self) -> bool {
        self.notify_options.removed.load(Ordering::Acquire)
    }

    pub fn get_tray_battery_icon_bt_id(&self) -> Option<&str> {
        match &self.tray_config.tray_icon_source {
            TrayIconSource::App => None,
            TrayIconSource::BatteryDefault(id) => Some(id),
            TrayIconSource::BatteryCustom(id) => Some(id),
        }
    }
}

#[derive(Debug)]
pub enum TrayIconSource {
    App,
    BatteryDefault(String), // Bluetooth ID
    BatteryCustom(String),  // Bluetooth ID
}

#[derive(Debug)]
pub struct TrayConfig {
    pub tooltip_options: TooltipOptions,
    pub tray_icon_source: TrayIconSource,
    pub update_interval: AtomicU64,
}

impl Default for TrayConfig {
    fn default() -> Self {
        TrayConfig {
            update_interval: AtomicU64::new(30),
            tray_icon_source: TrayIconSource::App,
            tooltip_options: TooltipOptions::default(),
        }
    }
}

impl TrayConfig {
    pub fn update(&mut self, name: &str, check: bool) {
        match name {
            "show_disconnected" => self
                .tooltip_options
                .show_disconnected
                .store(check, Ordering::Relaxed),
            "truncate_name" => self
                .tooltip_options
                .truncate_name
                .store(check, Ordering::Relaxed),
            "prefix_battery" => self
                .tooltip_options
                .prefix_battery
                .store(check, Ordering::Relaxed),
            _ => (),
        }
    }
}

#[derive(Default, Debug)]
pub struct TooltipOptions {
    pub show_disconnected: AtomicBool,
    pub truncate_name: AtomicBool,
    pub prefix_battery: AtomicBool,
}

#[derive(Debug)]
pub struct NotifyOptions {
    pub mute: AtomicBool,
    pub low_battery: AtomicU8,
    pub disconnection: AtomicBool,
    pub reconnection: AtomicBool,
    pub added: AtomicBool,
    pub removed: AtomicBool,
}

impl Default for NotifyOptions {
    fn default() -> Self {
        NotifyOptions {
            mute: AtomicBool::new(false),
            low_battery: AtomicU8::new(15),
            disconnection: AtomicBool::new(false),
            reconnection: AtomicBool::new(false),
            added: AtomicBool::new(false),
            removed: AtomicBool::new(false),
        }
    }
}

impl NotifyOptions {
    pub fn update(&mut self, name: &str, check: bool) {
        match name {
            "mute" => self.mute.store(check, Ordering::Relaxed),
            "disconnection" => self.disconnection.store(check, Ordering::Relaxed),
            "reconnection" => self.reconnection.store(check, Ordering::Relaxed),
            "added" => self.added.store(check, Ordering::Relaxed),
            "removed" => self.removed.store(check, Ordering::Relaxed),
            _ => (),
        }
    }
}

impl Config {
    pub fn oepn() -> Result<Self> {
        let config_path = env::current_exe()
            .ok()
            .and_then(|exe_path| exe_path.parent().map(Path::to_path_buf))
            .map(|parent_path| parent_path.join("BlueGauge.ini"))
            .ok_or(anyhow!("Failed to get config path"))?;

        if config_path.is_file() {
            Config::read_ini(config_path)
        } else {
            Config::create_ini(config_path)
        }
    }

    pub fn write_notify_options(&mut self, key: &str, value: &str) {
        let ini_path = &self.config_path;
        let mut ini = Ini::load_from_file(ini_path).expect("Failed to load BlueGauge.config");
        ini.set_to(Some("NotifyOptions"), key.to_owned(), value.to_owned());
        ini.write_to_file(ini_path)
            .expect("Failed to write config to BlueGauge.ini");
    }

    pub fn write_tray_config(&mut self, key: &str, value: &str) {
        let ini_path = &self.config_path;
        let mut ini = Ini::load_from_file(ini_path).expect("Failed to load BlueGauge.config");
        ini.set_to(Some("TrayConfig"), key.to_owned(), value.to_owned());
        ini.write_to_file(ini_path)
            .expect("Failed to write config to BlueGauge.ini");
    }

    fn create_ini(ini_path: PathBuf) -> Result<Self> {
        let mut ini = Ini::new();

        ini.with_section(Some("TrayConfig"))
            .set("update_interval", "30")
            .set("show_disconnected", "false")
            .set("truncate_name", "false")
            .set("prefix_battery", "false")
            .set("tray_icon_source", "app"); // app、id

        ini.with_section(Some("NotifyOptions"))
            .set("mute", "false")
            .set("low_battery", "15")
            .set("disconnection", "false")
            .set("reconnection", "false")
            .set("added", "false")
            .set("removed", "flase");

        ini.write_to_file(&ini_path)
            .with_context(|| "Failed to create BlueGauge.ini")?;

        Ok(Config {
            config_path: ini_path,
            update_config_event: AtomicBool::new(false),
            tray_config: TrayConfig::default(),
            notify_options: NotifyOptions::default(),
        })
    }

    fn read_ini(ini_path: PathBuf) -> Result<Self> {
        let ini = Ini::load_from_file(&ini_path).with_context(|| "Failed to load BlueGauge.ini")?;

        // 托盘设置
        let tray_config_section = ini
            .section(Some("TrayConfig"))
            .with_context(|| "Failed to get 'TrayConfig' Section")?;

        let update_interval = tray_config_section
            .get("update_interval")
            .filter(|v| !v.trim().is_empty())
            .and_then(|v| v.trim().parse::<u64>().ok())
            .unwrap_or(30);

        let show_disconnected = tray_config_section
            .get("show_disconnected")
            .is_some_and(|v| v.trim().to_lowercase() == "true");

        let truncate_name = tray_config_section
            .get("truncate_name")
            .is_some_and(|v| v.trim().to_lowercase() == "true");

        let prefix_battery = tray_config_section
            .get("prefix_battery")
            .is_some_and(|v| v.trim().to_lowercase() == "true");

        let tray_icon_source = match tray_config_section
            .get("tray_icon_source")
            .map(|s| s.trim())
            .as_deref()
        {
            Some("app") => TrayIconSource::App,
            Some(id) if !id.is_empty() => {
                let have_custom_icons = env::current_exe()
                    .ok()
                    .and_then(|exe_path| exe_path.parent().map(Path::to_path_buf))
                    .map(|p| (0..=100).all(|i| p.join(format!("assets\\{i}.png")).is_file()))
                    .unwrap_or(false);
                if have_custom_icons {
                    TrayIconSource::BatteryCustom(id.to_string())
                } else {
                    TrayIconSource::BatteryDefault(id.to_string())
                }
            }
            _ => TrayIconSource::App,
        };

        // 通知设置
        let notify_options_section = ini
            .section(Some("NotifyOptions"))
            .with_context(|| "Failed to get 'Notifications' Section")?;

        let mute = notify_options_section
            .get("mute")
            .is_some_and(|v| v.trim().to_lowercase() == "true");

        let low_battery = notify_options_section
            .get("low_battery")
            .and_then(|v| v.trim().parse::<u8>().ok())
            .filter(|&battery| battery <= 100)
            .unwrap_or(15);

        let disconnection = notify_options_section
            .get("disconnection")
            .is_some_and(|v| v.trim().to_lowercase() == "true");

        let reconnection = notify_options_section
            .get("reconnection")
            .is_some_and(|v| v.trim().to_lowercase() == "true");

        let added = notify_options_section
            .get("added")
            .is_some_and(|v| v.trim().to_lowercase() == "true");

        let removed = notify_options_section
            .get("removed")
            .is_some_and(|v| v.trim().to_lowercase() == "true");

        Ok(Config {
            config_path: ini_path,
            update_config_event: AtomicBool::new(false),
            tray_config: TrayConfig {
                tooltip_options: TooltipOptions {
                    show_disconnected: AtomicBool::new(show_disconnected),
                    truncate_name: AtomicBool::new(truncate_name),
                    prefix_battery: AtomicBool::new(prefix_battery),
                },
                tray_icon_source,
                update_interval: AtomicU64::new(update_interval),
            },
            notify_options: NotifyOptions {
                mute: AtomicBool::new(mute),
                low_battery: AtomicU8::new(low_battery),
                disconnection: AtomicBool::new(disconnection),
                reconnection: AtomicBool::new(reconnection),
                added: AtomicBool::new(added),
                removed: AtomicBool::new(removed),
            },
        })
    }
}
