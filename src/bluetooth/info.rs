use crate::{
    bluetooth::{
        ble::{find_ble_devices, get_ble_info},
        btc::{find_btc_devices, get_btc_info},
    },
    config::Config,
    language::{Language, Localization},
    notify::{app_notify, notify},
};

use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
};

use anyhow::{Result, anyhow};
use windows::Devices::Bluetooth::{BluetoothDevice, BluetoothLEDevice};

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub enum BluetoothType {
    Classic(/* Instance ID */ String, /* Address */ u64),
    LowEnergy(/* Address */ u64),
}

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct BluetoothInfo {
    pub name: String,
    pub battery: u8,
    pub status: bool,
    pub address: String,
    pub r#type: BluetoothType,
}

pub fn find_bluetooth_devices() -> Result<(Vec<BluetoothDevice>, Vec<BluetoothLEDevice>)> {
    let bt_devices = find_btc_devices()?;
    let ble_devices = find_ble_devices()?;
    Ok((bt_devices, ble_devices))
}

pub fn get_bluetooth_info(
    bt_devices: (&[BluetoothDevice], &[BluetoothLEDevice]),
) -> Result<HashSet<BluetoothInfo>> {
    let btc_devices = bt_devices.0;
    let ble_devices = bt_devices.1;
    match (btc_devices.len(), ble_devices.len()) {
        (0, 0) => Err(anyhow!(
            "No Classic Bluetooth and Bluetooth LE devices found"
        )),
        (0, _) => dbg!(get_ble_info(&ble_devices).or_else(|e| {
            app_notify(format!("Warning: Failed to get BLE info: {e}"));
            Ok(HashSet::new())
        })),
        (_, 0) => dbg!(get_btc_info(&btc_devices).or_else(|e| {
            app_notify(format!("Warning: Failed to get BTC info: {e}"));
            Ok(HashSet::new())
        })),
        (_, _) => {
            let btc_result = dbg!(get_btc_info(&btc_devices));
            let ble_result = dbg!(get_ble_info(&ble_devices));

            match (btc_result, ble_result) {
                (Ok(btc_info), Ok(ble_info)) => {
                    let combined_info = btc_info.into_iter().chain(ble_info).collect();
                    Ok(combined_info)
                }
                (Ok(btc_info), Err(e)) => {
                    println!("Warning: Failed to get BLE info: {e}");
                    Ok(btc_info)
                }
                (Err(e), Ok(ble_info)) => {
                    println!("Warning: Failed to get BTC info: {e}");
                    Ok(ble_info)
                }
                (Err(btc_err), Err(ble_err)) => Err(anyhow!(
                    "Failed to get both BTC and BLE info: {btc_err} | {ble_err}"
                )),
            }
        }
    }
}

pub fn compare_bt_info_to_send_notifications(
    config: &Config,
    notified_low_battery: Arc<Mutex<HashSet<String>>>,
    old_bt_info: Arc<Mutex<HashSet<BluetoothInfo>>>,
    new_bt_info: &HashSet<BluetoothInfo>,
) -> Option<Result<()>> {
    let mut old_bt_info = old_bt_info.lock().unwrap();

    let change_old_bt_info = old_bt_info
        .difference(new_bt_info)
        .cloned()
        .collect::<HashSet<_>>();
    let change_new_bt_info = new_bt_info
        .difference(&old_bt_info)
        .cloned()
        .collect::<HashSet<_>>();

    if change_old_bt_info == change_new_bt_info {
        return None;
    }

    let low_battery = config.get_low_battery();
    let mute = config.get_mute();
    let disconnection = config.get_disconnection();
    let reconnection = config.get_reconnection();
    let added = config.get_added();
    let removed = config.get_removed();

    std::thread::spawn(move || {
        let language = Language::get_system_language();
        let loc = Localization::get(language);

        let mut notified_low_battery = notified_low_battery.lock().unwrap();

        for old in &change_old_bt_info {
            for new in &change_new_bt_info {
                // 低电量 / 重新连接 / 断开连接 的同一设备
                if old.address == new.address {
                    if new.battery != old.battery {
                        let is_low = new.battery < low_battery;
                        let was_low = notified_low_battery.contains(&new.address);
                        match (was_low, is_low) {
                            (false, true) => {
                                // 第一次进入低电量
                                let title =
                                    format!("{} {low_battery}%", loc.bluetooth_battery_below);
                                let text = format!("{}: {}%", new.name, new.battery);
                                notify(title, text, mute);
                                notified_low_battery.insert(new.address.clone());
                            }
                            (true, false) => {
                                // 电量回升，允许下次低电量时再次通知
                                notified_low_battery.remove(&new.address);
                            }
                            _ => (),
                        }
                    }

                    if new.status != old.status {
                        if disconnection && !new.status {
                            notify(
                                loc.bluetooth_device_disconnected,
                                format!("{}: {}", loc.device_name, new.name),
                                mute,
                            );
                        }

                        if reconnection && new.status {
                            notify(
                                loc.bluetooth_device_reconnected,
                                format!("{}: {}", loc.device_name, new.name),
                                mute,
                            );
                        }
                    }

                    continue;
                }

                // 新添加设备
                if added {
                    let added_devices = change_new_bt_info
                        .difference(&change_old_bt_info)
                        .collect::<HashSet<_>>();
                    if !added_devices.is_empty() {
                        notify(
                            loc.new_bluetooth_device_add,
                            format!("{}: {}", loc.device_name, new.name),
                            mute,
                        );
                    }
                }

                // 移除设备
                if removed {
                    let removed_devices = change_old_bt_info
                        .difference(&change_new_bt_info)
                        .collect::<HashSet<_>>();
                    if !removed_devices.is_empty() {
                        notify(
                            loc.old_bluetooth_device_removed,
                            format!("{}: {}", loc.device_name, old.name),
                            mute,
                        );
                    }
                }
            }
        }
    });

    *old_bt_info = new_bt_info.clone();

    Some(Ok(()))
}
