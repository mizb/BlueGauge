use crate::{
    config::Config,
    language::{Language, Localization},
    notify::{app_notify, notify},
};

use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
};

use anyhow::{Context, Result, anyhow};
use windows::{
    Devices::Bluetooth::{
        BluetoothConnectionStatus as BCS, BluetoothDevice, BluetoothLEDevice,
        GenericAttributeProfile::{GattCharacteristicUuids, GattServiceUuids},
    },
    Devices::Enumeration::DeviceInformation,
    Storage::Streams::DataReader,
    core::GUID,
};
use windows_pnp::{PnpDeviceNodeInfo, PnpDevicePropertyValue, PnpEnumerator};
use windows_sys::{
    Wdk::Devices::Bluetooth::DEVPKEY_Bluetooth_DeviceAddress,
    Win32::{Devices::DeviceAndDriverInstallation::GUID_DEVCLASS_SYSTEM, Foundation::DEVPROPKEY},
};

#[allow(non_upper_case_globals)]
const DEVPKEY_Bluetooth_Battery: DEVPROPKEY = DEVPROPKEY {
    fmtid: windows_sys::core::GUID::from_u128(0x104EA319_6EE2_4701_BD47_8DDBF425BBE5),
    pid: 2,
};
const BT_INSTANCE_ID: &str = "BTHENUM\\";

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct BluetoothInfo {
    pub name: String,
    pub battery: u8,
    pub status: bool,
    pub id: String,
}

pub fn find_bluetooth_devices() -> Result<(Vec<BluetoothDevice>, Vec<BluetoothLEDevice>)> {
    let bt_devices = find_btc_devices()?;
    let ble_devices = find_ble_devices()?;
    Ok((bt_devices, ble_devices))
}

// Bluetooth Classic
fn find_btc_devices() -> Result<Vec<BluetoothDevice>> {
    let btc_aqs_filter = BluetoothDevice::GetDeviceSelectorFromPairingState(true)?;

    let btc_devices_info = DeviceInformation::FindAllAsyncAqsFilter(&btc_aqs_filter)?
        .get()
        .with_context(|| "Faled to find Bluetooth Classic from all devices")?;

    let btc_devices = btc_devices_info
        .into_iter()
        .filter_map(|device_info| {
            BluetoothDevice::FromIdAsync(&device_info.Id().ok()?)
                .ok()?
                .get()
                .ok()
        })
        .collect::<Vec<_>>();

    Ok(btc_devices)
}

// Bluetooth LE
fn find_ble_devices() -> Result<Vec<BluetoothLEDevice>> {
    let ble_aqs_filter = BluetoothLEDevice::GetDeviceSelectorFromPairingState(true)?;

    let ble_devices_info = DeviceInformation::FindAllAsyncAqsFilter(&ble_aqs_filter)?
        .get()
        .with_context(|| "Faled to find Bluetooth Low Energy from all devices")?;

    let ble_devices = ble_devices_info
        .into_iter()
        .filter_map(|device_info| {
            BluetoothLEDevice::FromIdAsync(&device_info.Id().ok()?)
                .ok()?
                .get()
                .ok()
        })
        .collect::<Vec<_>>();

    Ok(ble_devices)
}

pub fn get_bluetooth_info(
    bt_devices: (Vec<BluetoothDevice>, Vec<BluetoothLEDevice>),
) -> Result<HashSet<BluetoothInfo>> {
    let btc_devices = bt_devices.0;
    let ble_devices = bt_devices.1;
    match (btc_devices.len(), ble_devices.len()) {
        (0, 0) => Err(anyhow!(
            "No Classic Bluetooth and Bluetooth LE devices found"
        )),
        (0, _) => dbg!(get_ble_info(ble_devices).or_else(|e| {
            app_notify(format!("Warning: Failed to get BLE info: {e}"));
            Ok(HashSet::new())
        })),
        (_, 0) => dbg!(get_btc_info(btc_devices).or_else(|e| {
            app_notify(format!("Warning: Failed to get BTC info: {e}"));
            Ok(HashSet::new())
        })),
        (_, _) => {
            let btc_result = dbg!(get_btc_info(btc_devices));
            let ble_result = dbg!(get_ble_info(ble_devices));

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

fn get_btc_info(btc_devices: Vec<BluetoothDevice>) -> Result<HashSet<BluetoothInfo>> {
    // 获取Pnp设备可能出错（具有不详），需重试多次避开错误
    let pnp_btc_devices_info: Vec<(String, u8)> = {
        let max_retries = 2;
        let mut attempts = 0;

        loop {
            match get_pnp_btc_devices_info() {
                Ok(info) => break info,
                Err(e) => {
                    attempts += 1;
                    if attempts >= max_retries {
                        return Err(anyhow!(
                            "Trying to enumerate the pnp device twice failed: {e}"
                        )); // 达到最大重试次数，返回错误
                    }
                    println!(
                        "Failed to get Bluetooth device information: {e}, try again after 2 seconds... (try {attempts}/{max_retries})"
                    );
                    std::thread::sleep(std::time::Duration::from_secs(2));
                }
            }
        }
    };

    let mut devices_info: HashSet<BluetoothInfo> = HashSet::new();

    btc_devices.into_iter().for_each(|btc_device| {
        let _ = process_btc_device(btc_device, &pnp_btc_devices_info)
            .inspect_err(|e| println!("\n{e}\n"))
            .is_ok_and(|bt_info| devices_info.insert(bt_info));
    });

    Ok(devices_info)
}

fn get_ble_info(ble_devices: Vec<BluetoothLEDevice>) -> Result<HashSet<BluetoothInfo>> {
    let mut devices_info: HashSet<BluetoothInfo> = HashSet::new();

    let results = ble_devices.iter().map(process_ble_device);

    results.into_iter().for_each(|r_ble_info| {
        let _ = r_ble_info
            .inspect_err(|e| println!("\n{e}\n"))
            .is_ok_and(|bt_info| devices_info.insert(bt_info));
    });

    Ok(devices_info)
}

fn process_btc_device(
    btc_device: BluetoothDevice,
    pnp_btc_devices_info: &[(String, u8)],
) -> Result<BluetoothInfo> {
    let btc_name: String = btc_device.Name()?.to_string().trim().into();

    let btc_address = format!("{:012X}", btc_device.BluetoothAddress()?);

    let btc_battery = pnp_btc_devices_info
        .iter()
        .find_map(|(pnp_address, pnp_battery)| btc_address.eq(pnp_address).then_some(*pnp_battery))
        .ok_or(anyhow!(
            "No matching Bluetooth Classic Device in Pnp device: {btc_name}"
        ))?;

    let btc_status = btc_device.ConnectionStatus()? == BCS::Connected;
    Ok(BluetoothInfo {
        name: btc_name,
        battery: btc_battery,
        status: btc_status,
        id: btc_address,
    })
}

fn process_ble_device(ble_device: &BluetoothLEDevice) -> Result<BluetoothInfo> {
    let name = ble_device.Name()?.to_string();

    let battery = get_ble_battery_level(ble_device)
        .map_err(|e| anyhow!("Failed to get '{name}'BLE Battery Level: {e}"))?;

    let status = ble_device
        .ConnectionStatus()
        .map(|status| matches!(status, BCS::Connected))
        .with_context(|| format!("Failed to get BLE connected status: {name}"))?;

    let id = format!("{:012X}", ble_device.BluetoothAddress()?);

    Ok(BluetoothInfo {
        name,
        battery,
        status,
        id,
    })
}

fn get_ble_battery_level(ble_device: &BluetoothLEDevice) -> Result<u8> {
    // 0000180F-0000-1000-8000-00805F9B34FB
    let battery_services_uuid: GUID = GattServiceUuids::Battery()?;
    // 00002A19-0000-1000-8000-00805F9B34FB
    let battery_level_uuid: GUID = GattCharacteristicUuids::BatteryLevel()?;

    // windows-rs库的GetGattServicesForUuidAsync异步与tray-icon的异步（托盘点击事件？）可能存在冲突进而导致阻塞
    let battery_gatt_service = ble_device
        .GetGattService(battery_services_uuid)
        .map_err(|e| anyhow!("Failed to get BLE Battery Gatt Service: {e}"))?; // 手机蓝牙无电量服务;

    let battery_gatt_chars = battery_gatt_service
        .GetCharacteristicsForUuidAsync(battery_level_uuid)?
        .get()?
        .Characteristics()
        .map_err(|e| anyhow!("Failed to get BLE Battery Gatt Characteristics: {e}"))?;

    let battery_gatt_char = battery_gatt_chars
        .into_iter()
        .next()
        .ok_or_else(|| anyhow!("Failed to get BLE Battery Gatt Characteristic"))?;

    match battery_gatt_char.Uuid()? == battery_level_uuid {
        true => {
            let buffer = battery_gatt_char.ReadValueAsync()?.get()?.Value()?;
            let reader = DataReader::FromBuffer(&buffer)?;
            reader
                .ReadByte()
                .map_err(|e| anyhow!("Failed to read byte: {e}"))
        }
        false => Err(anyhow!(
            "Failed to match BLE level UUID:\n{:?}:\n{battery_level_uuid:?}",
            battery_gatt_char.Uuid()?
        )),
    }
}

fn get_pnp_btc_devices_info() -> Result<Vec<(String, u8)>> {
    let mut pnp_btc_devices_info: Vec<(String, u8)> = Vec::new();

    let bt_devices_info = get_pnp_bt_devices(GUID_DEVCLASS_SYSTEM)?;

    for bt_device_info in bt_devices_info {
        if !bt_device_info.device_instance_id.contains(BT_INSTANCE_ID) {
            continue;
        }

        if let Some(mut props) = bt_device_info.device_instance_properties {
            let battery = props
                .remove(&DEVPKEY_Bluetooth_Battery.into())
                .and_then(|value| match value {
                    PnpDevicePropertyValue::Byte(v) => Some(v),
                    _ => None,
                });

            let address = props
                .remove(&DEVPKEY_Bluetooth_DeviceAddress.into())
                .and_then(|value| match value {
                    PnpDevicePropertyValue::String(v) => Some(v),
                    _ => None,
                });

            if let (Some(address), Some(battery)) = (address, battery) {
                pnp_btc_devices_info.push((address, battery));
            }
        }
    }

    Ok(pnp_btc_devices_info)
}

fn get_pnp_bt_devices(guid: windows_sys::core::GUID) -> Result<Vec<PnpDeviceNodeInfo>> {
    PnpEnumerator::enumerate_present_devices_by_device_setup_class(guid)
        .map_err(|e| anyhow!("Failed to enumerate pnp devices - {e:?}"))
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
                if old.id == new.id {
                    if new.battery != old.battery {
                        let is_low = new.battery < low_battery;
                        let was_low = notified_low_battery.contains(&new.id);
                        match (was_low, is_low) {
                            (false, true) => {
                                // 第一次进入低电量
                                let title =
                                    format!("{} {low_battery}%", loc.bluetooth_battery_below);
                                let text = format!("{}: {}%", new.name, new.battery);
                                notify(title, text, mute);
                                notified_low_battery.insert(new.id.clone());
                            }
                            (true, false) => {
                                // 电量回升，允许下次低电量时再次通知
                                notified_low_battery.remove(&new.id);
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
