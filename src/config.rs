use std::env;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};

use crate::notify::app_notify;

use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
struct TomlConfig {
    #[serde(rename = "TrayConfig")]
    tray_config: TrayConfigToml,

    #[serde(rename = "NotifyOptions")]
    notify_options: NotifyOptionsToml,
}

#[derive(Debug, Serialize, Deserialize)]
struct TrayConfigToml {
    update_interval: u64,
    show_disconnected: bool,
    truncate_name: bool,
    prefix_battery: bool,

    #[serde(rename = "TrayIconSource")]
    tray_icon_source: TrayIconSource,
}

#[derive(Debug, Serialize, Deserialize)]
struct NotifyOptionsToml {
    mute: bool,
    low_battery: u8,
    disconnection: bool,
    reconnection: bool,
    added: bool,
    removed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "config")]
pub enum TrayIconSource {
    App,
    BatteryCustom {
        id: String,
    },
    BatteryFont {
        id: String,
        font_name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        font_color: Option<String>,
    },
}

impl TrayIconSource {
    pub fn update_id(&mut self, new_id: &str) {
        match self {
            Self::App => (),
            Self::BatteryCustom { id } => *id = new_id.to_string(),
            Self::BatteryFont { id, .. } => *id = new_id.to_string(),
        }
    }
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
    pub fn update(&self, name: &str, check: bool) {
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

#[derive(Default, Debug)]
pub struct TooltipOptions {
    pub show_disconnected: AtomicBool,
    pub truncate_name: AtomicBool,
    pub prefix_battery: AtomicBool,
}

#[derive(Debug)]
pub struct TrayConfig {
    pub tooltip_options: TooltipOptions,
    pub tray_icon_source: Mutex<TrayIconSource>,
    pub update_interval: AtomicU64,
}

impl Default for TrayConfig {
    fn default() -> Self {
        TrayConfig {
            update_interval: AtomicU64::new(60),
            tray_icon_source: Mutex::new(TrayIconSource::App),
            tooltip_options: TooltipOptions::default(),
        }
    }
}

impl TrayConfig {
    pub fn update(&self, name: &str, check: bool) {
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

#[derive(Debug)]
pub struct Config {
    pub config_path: PathBuf,
    pub notify_options: NotifyOptions,
    pub tray_config: TrayConfig,
    pub force_update: AtomicBool,
}

impl Config {
    pub fn open() -> Result<Self> {
        let config_path = env::current_exe()
            .ok()
            .and_then(|exe_path| exe_path.parent().map(Path::to_path_buf))
            .map(|parent_path| parent_path.join("BlueGauge.toml"))
            .ok_or(anyhow!("Failed to get config path"))?;

        if config_path.is_file() {
            Config::read_toml(config_path.clone()).or_else(|e| {
                app_notify(format!("Failed to read config file: {e}"));
                Config::create_toml(config_path)
            })
        } else {
            Config::create_toml(config_path)
        }
    }

    pub fn save(&self) {
        let tray_icon_source = {
            let lock = self.tray_config.tray_icon_source.lock().unwrap();
            lock.clone()
        };
        let toml_config = TomlConfig {
            tray_config: TrayConfigToml {
                update_interval: self.tray_config.update_interval.load(Ordering::Relaxed),
                show_disconnected: self
                    .tray_config
                    .tooltip_options
                    .show_disconnected
                    .load(Ordering::Relaxed),
                truncate_name: self
                    .tray_config
                    .tooltip_options
                    .truncate_name
                    .load(Ordering::Relaxed),
                prefix_battery: self
                    .tray_config
                    .tooltip_options
                    .prefix_battery
                    .load(Ordering::Relaxed),
                tray_icon_source,
            },
            notify_options: NotifyOptionsToml {
                mute: self.notify_options.mute.load(Ordering::Relaxed),
                low_battery: self.notify_options.low_battery.load(Ordering::Relaxed),
                disconnection: self.notify_options.disconnection.load(Ordering::Relaxed),
                reconnection: self.notify_options.reconnection.load(Ordering::Relaxed),
                added: self.notify_options.added.load(Ordering::Relaxed),
                removed: self.notify_options.removed.load(Ordering::Relaxed),
            },
        };

        let toml_str = toml::to_string_pretty(&toml_config)
            .expect("Failed to serialize TomlConfig structure as a String of TOML.");
        std::fs::write(&self.config_path, toml_str)
            .expect("Failed to TOML String to BlueGauge.toml");
    }

    fn create_toml(config_path: PathBuf) -> Result<Self> {
        let default_config = TomlConfig {
            tray_config: TrayConfigToml {
                update_interval: 60,
                show_disconnected: false,
                truncate_name: false,
                prefix_battery: false,
                tray_icon_source: TrayIconSource::App,
            },
            notify_options: NotifyOptionsToml {
                mute: false,
                low_battery: 15,
                disconnection: false,
                reconnection: false,
                added: false,
                removed: false,
            },
        };

        let toml_str = toml::to_string_pretty(&default_config)?;
        std::fs::write(&config_path, toml_str)?;

        Ok(Config {
            config_path,
            force_update: AtomicBool::new(false),
            tray_config: TrayConfig {
                update_interval: AtomicU64::new(default_config.tray_config.update_interval),
                tray_icon_source: Mutex::new(default_config.tray_config.tray_icon_source),
                tooltip_options: TooltipOptions {
                    show_disconnected: AtomicBool::new(
                        default_config.tray_config.show_disconnected,
                    ),
                    truncate_name: AtomicBool::new(default_config.tray_config.truncate_name),
                    prefix_battery: AtomicBool::new(default_config.tray_config.prefix_battery),
                },
            },
            notify_options: NotifyOptions {
                mute: AtomicBool::new(default_config.notify_options.mute),
                low_battery: AtomicU8::new(default_config.notify_options.low_battery),
                disconnection: AtomicBool::new(default_config.notify_options.disconnection),
                reconnection: AtomicBool::new(default_config.notify_options.reconnection),
                added: AtomicBool::new(default_config.notify_options.added),
                removed: AtomicBool::new(default_config.notify_options.removed),
            },
        })
    }

    fn read_toml(config_path: PathBuf) -> Result<Self> {
        let content = std::fs::read_to_string(&config_path)?;
        let toml_config: TomlConfig = toml::from_str(&content)?;

        Ok(Config {
            config_path,
            force_update: AtomicBool::new(false),
            tray_config: TrayConfig {
                update_interval: AtomicU64::new(toml_config.tray_config.update_interval),
                tray_icon_source: Mutex::new(toml_config.tray_config.tray_icon_source),
                tooltip_options: TooltipOptions {
                    show_disconnected: AtomicBool::new(toml_config.tray_config.show_disconnected),
                    truncate_name: AtomicBool::new(toml_config.tray_config.truncate_name),
                    prefix_battery: AtomicBool::new(toml_config.tray_config.prefix_battery),
                },
            },
            notify_options: NotifyOptions {
                mute: AtomicBool::new(toml_config.notify_options.mute),
                low_battery: AtomicU8::new(toml_config.notify_options.low_battery),
                disconnection: AtomicBool::new(toml_config.notify_options.disconnection),
                reconnection: AtomicBool::new(toml_config.notify_options.reconnection),
                added: AtomicBool::new(toml_config.notify_options.added),
                removed: AtomicBool::new(toml_config.notify_options.removed),
            },
        })
    }
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

    pub fn get_tray_battery_icon_bt_id(&self) -> Option<String> {
        let tray_icon_source = {
            let lock = self.tray_config.tray_icon_source.lock().unwrap();
            lock.clone()
        };

        match tray_icon_source {
            TrayIconSource::App => None,
            TrayIconSource::BatteryCustom { id } => Some(id),
            TrayIconSource::BatteryFont { id, .. } => Some(id),
        }
    }
}
