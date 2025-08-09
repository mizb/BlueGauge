use crate::bluetooth::info::{BluetoothInfo, BluetoothType};

use std::collections::HashSet;

use anyhow::{Context, Result, anyhow};
use windows::{
    Devices::Bluetooth::{
        BluetoothConnectionStatus, BluetoothLEDevice,
        GenericAttributeProfile::{GattCharacteristicUuids, GattServiceUuids},
    },
    Devices::Enumeration::DeviceInformation,
    Storage::Streams::DataReader,
    core::GUID,
};

pub fn find_ble_devices() -> Result<Vec<BluetoothLEDevice>> {
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

pub fn find_ble_device(address: u64) -> Result<BluetoothLEDevice> {
    BluetoothLEDevice::FromBluetoothAddressAsync(address)?.
        get()
        .map_err(|e| anyhow!("Failed to find ble ({address}) - {e}"))
}

pub fn get_ble_info(ble_devices: Vec<BluetoothLEDevice>) -> Result<HashSet<BluetoothInfo>> {
    let mut devices_info: HashSet<BluetoothInfo> = HashSet::new();

    let results = ble_devices.iter().map(process_ble_device);

    results.into_iter().for_each(|r_ble_info| {
        let _ = r_ble_info
            .inspect_err(|e| println!("\n{e}\n"))
            .is_ok_and(|bt_info| devices_info.insert(bt_info));
    });

    Ok(devices_info)
}

pub fn process_ble_device(ble_device: &BluetoothLEDevice) -> Result<BluetoothInfo> {
    let name = ble_device.Name()?.to_string();

    let battery = get_ble_battery_level(ble_device)
        .map_err(|e| anyhow!("Failed to get '{name}'BLE Battery Level: {e}"))?;

    let status = ble_device
        .ConnectionStatus()
        .map(|status| status == BluetoothConnectionStatus::Connected)
        .with_context(|| format!("Failed to get BLE connected status: {name}"))?;

    let address_u64 = ble_device.BluetoothAddress()?;
    let address_string = format!("{address_u64:012X}");

    Ok(BluetoothInfo {
        name,
        battery,
        status,
        address: address_string,
        r#type: BluetoothType::LowEnergy(address_u64),
    })
}

pub fn get_ble_battery_level(ble_device: &BluetoothLEDevice) -> Result<u8> {
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
