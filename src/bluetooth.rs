use windows::{
    core::{GUID, Error, HRESULT},
    Devices::Bluetooth::GenericAttributeProfile::{GattCharacteristicUuids, GattServiceUuids},
    Devices::Bluetooth::{BluetoothConnectionStatus as BCS, BluetoothDevice, BluetoothLEDevice},
    Devices::Enumeration::DeviceInformation,
    Storage::Streams::DataReader,
};
use anyhow::{anyhow, Context, Result};
use futures::future::join_all;
use scalefs_windowspnp::{PnpDeviceNodeInfo, PnpDevicePropertyValue, PnpEnumerator};
use windows_sys::Win32::Devices::DeviceAndDriverInstallation::GUID_DEVCLASS_SYSTEM;
use windows_sys::Win32::Devices::Properties::{DEVPKEY_Device_FriendlyName, DEVPROPKEY};

#[allow(non_upper_case_globals)]
const DEVPKEY_Bluetooth_Battery: DEVPROPKEY = DEVPROPKEY {
    fmtid: windows_sys::core::GUID::from_u128(0x104EA319_6EE2_4701_BD47_8DDBF425BBE5),
    pid: 2,
};
const BT_INSTANCE_ID: &str = "BTHENUM\\";

pub struct BluetoothInfo {
    pub name: String,
    pub battery: u8,
    pub status: bool,
}

pub async fn find_bluetooth_devices(
) -> Result<(Vec<BluetoothDevice>, Vec<BluetoothLEDevice>)> {
    // 获取已配对蓝牙设备的过滤器（AQS 查询字符串）
    let bt_aqs_filter = BluetoothDevice::GetDeviceSelectorFromPairingState(true)?;
    let ble_aqs_filter = BluetoothLEDevice::GetDeviceSelectorFromPairingState(true)?;

    let bt_devices_info_collection = DeviceInformation::FindAllAsyncAqsFilter(&bt_aqs_filter)?
        .await
        .context("Faled to find Bluetooth Classic from all devices")?;
    let ble_devices_info_collection = DeviceInformation::FindAllAsyncAqsFilter(&ble_aqs_filter)?
        .await
        .context("Faled to find Bluetooth Low Energy from all devices")?;
    
    let bt_devices_futures = bt_devices_info_collection.into_iter().map(|device_info| {
        async move { BluetoothDevice::FromIdAsync(&device_info.Id().ok()?).ok()?.await.ok() }
    }).collect::<Vec<_>>();
    let ble_devices_futures = ble_devices_info_collection.into_iter().map(|device_info| {
        async move { BluetoothLEDevice::FromIdAsync(&device_info.Id().ok()?).ok()?.await.ok() }
    }).collect::<Vec<_>>();

    let bt_devices: Vec<_> = join_all(bt_devices_futures).await.into_iter().filter_map(|x| x).collect();
    let ble_devices: Vec<_> = join_all(ble_devices_futures).await.into_iter().filter_map(|x| x).collect();

    Ok((bt_devices, ble_devices))
}

pub async fn get_bluetooth_info(
    bt_devices: Vec<BluetoothDevice>,
    ble_devices: Vec<BluetoothLEDevice>,
) -> Result<Vec<BluetoothInfo>> {
    let mut devices_info: Vec<BluetoothInfo> = Vec::new();

    if bt_devices.len() > 0 {
        let pnp_bt_devices_info: Vec<(String, u8)> = get_pnp_bt_devices_info().await?;
        let futures = bt_devices.iter().map(|bt_device| {
            process_bt_device(bt_device, &pnp_bt_devices_info)
        });
        let results: Vec<Result<BluetoothInfo>> = join_all(futures).await;
        for result in results {
            match result {
                Ok(bt_info) => devices_info.push(bt_info),
                Err(e) => println!("{e}"),
            }
        }
    };

    if ble_devices.len() > 0 {
        let futures = ble_devices.iter().map(|bt_device| {
            process_ble_device(bt_device)
        });
        let results: Vec<Result<BluetoothInfo>> = join_all(futures).await;
        for result in results {
            match result {
                Ok(bt_info) => devices_info.push(bt_info),
                Err(e) => println!("{e}"),
            }
        }
    };

    Ok(devices_info)
}

async fn process_bt_device(
    bt_device: &BluetoothDevice,
    pnp_bt_devices_info: &[(String, u8)]
) -> Result<BluetoothInfo> {
    let bt_name = bt_device.Name()?.to_string();
    for (pnp_name, battery) in pnp_bt_devices_info {
        if pnp_name.contains(&bt_name) {            
            return Ok(BluetoothInfo {
                name: bt_name,
                battery: *battery,
                status: bt_device.ConnectionStatus()? == BCS::Connected
            });
        }
    }
    Err(anyhow!("No matching Bluetooth Classic found in Pnp device: {bt_name}"))
}

async fn process_ble_device(ble_device: &BluetoothLEDevice) -> Result<BluetoothInfo> {
    let name = ble_device.Name()?.to_string();
    let battery = get_ble_battery_level(&ble_device).await
        .map_err(|e| anyhow!("Failed to get BLE:'{name}' Battery Level -> {e}"))?;
    let status = ble_device
        .ConnectionStatus()
        .map(|status| matches!(status, BCS::Connected))
        .context(format!("Failed to get Bluetooth connected status: {name}"))?;
    
    Ok(BluetoothInfo { name, battery, status })
}

async fn get_ble_battery_level(bt_le_device: &BluetoothLEDevice) -> Result<u8> {
    let battery_services_uuid: GUID = GattServiceUuids::Battery()?;
    let battery_level_uuid: GUID = GattCharacteristicUuids::BatteryLevel()?;

    let battery_services = bt_le_device
        .GetGattServicesForUuidAsync(battery_services_uuid)?.await?
        .Services()
        .context("Failed to get BLE Services")?;

    let battery_service = battery_services
        .into_iter()
        .next()
        .ok_or_else(|| anyhow!("Failed to get BLE Service"))?; // 例如无法获取手机电池服务

    let battery_gatt_chars = battery_service
        .GetCharacteristicsForUuidAsync(battery_level_uuid)?.await?
        .Characteristics()
        .context("Failed to get BLE Gatt Characteristics")?;

    let battery_gatt_char = battery_gatt_chars
        .into_iter()
        .next()
        .ok_or_else(|| anyhow!("Failed to get BLE Gatt Characteristic"))?;

    let battery_level = match battery_gatt_char.Uuid()? == battery_level_uuid {
        true => {
            let buffer = battery_gatt_char.ReadValueAsync()?.await?.Value()?;
            let reader = DataReader::FromBuffer(&buffer)?;
            reader.ReadByte()
        }
        false => Err(Error::new(
            HRESULT(0x80004005u32 as i32),
            format!("Failed to match BLE level UUID: {:?}", battery_gatt_char.Uuid()?))
        ),
    };

    return battery_level.map_err(|e| anyhow!(e));
}

async fn get_pnp_bt_devices_info() -> Result<Vec<(String, u8)>> {
    let mut pnp_bt_devices_info: Vec<(String, u8)> = Vec::new();

    let bt_devices_info = get_pnp_bt_devices(GUID_DEVCLASS_SYSTEM).await?;
    for bt_device_info in bt_devices_info {
        if !bt_device_info.device_instance_id.contains(BT_INSTANCE_ID) {
            continue;
        }

        if let Some(props) = bt_device_info.device_instance_properties {
            let (mut name, mut battery_level) = (None, None);
            for (key, value) in props {
                if key == DEVPKEY_Device_FriendlyName.into() {
                    if let PnpDevicePropertyValue::String(v) = value {
                        name = Some(v)
                    }
                } else if key == DEVPKEY_Bluetooth_Battery.into() {
                    if let PnpDevicePropertyValue::Byte(v) = value {
                        battery_level = Some(v)
                    }
                }

                if let (Some(n), Some(b)) = (name.to_owned(), battery_level) {
                    pnp_bt_devices_info.push((n, b));
                    break;
                }
            }
        }
    }

    Ok(pnp_bt_devices_info)
}

async fn get_pnp_bt_devices(guid: windows_sys::core::GUID) -> Result<Vec<PnpDeviceNodeInfo>> {
    tokio::task::spawn_blocking(move || {
        PnpEnumerator::enumerate_present_devices_by_device_setup_class(guid)
            .map_err(|_| anyhow!("Failed to enumerate pnp devices"))
    })
    .await?
}
