use crate::bluetooth::info::{BluetoothInfo, BluetoothType};

use std::collections::{HashMap, HashSet};

use anyhow::{Context, Result, anyhow};
use windows::Devices::{
    Bluetooth::{BluetoothConnectionStatus, BluetoothDevice},
    Enumeration::DeviceInformation,
};
use windows_pnp::{
    DeviceInstanceIdFilter, PnpDeviceNodeInfo, PnpDevicePropertyValue, PnpEnumerator,
};
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

pub struct PnpDeviceInfo {
    pub address: u64,
    pub battery: u8,
    pub instance_id: String,
}

pub fn find_btc_devices() -> Result<Vec<BluetoothDevice>> {
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

pub fn find_btc_device(address: u64) -> Result<BluetoothDevice> {
    BluetoothDevice::FromBluetoothAddressAsync(address)?
        .get()
        .map_err(|e| anyhow!("Failed to find btc ({address}) - {e}"))
}

pub fn get_btc_info(btc_devices: &[BluetoothDevice]) -> Result<HashSet<BluetoothInfo>> {
    // 获取Pnp设备可能出错（初始化可能失败），需重试多次避开错误
    let pnp_devices_info = {
        let max_retries = 2;
        let mut attempts = 0;

        loop {
            match get_pnp_devices_info() {
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

    btc_devices.iter().for_each(|btc_device| {
        let _ = process_btc_device(btc_device, &pnp_devices_info)
            .inspect_err(|e| println!("\n{e}\n"))
            .is_ok_and(|bt_info| devices_info.insert(bt_info));
    });

    Ok(devices_info)
}

pub fn process_btc_device(
    btc_device: &BluetoothDevice,
    pnp_devices_info: &HashMap<u64, PnpDeviceInfo>,
) -> Result<BluetoothInfo> {
    let btc_name = btc_device.Name()?.to_string().trim().to_owned();

    let btc_address = btc_device.BluetoothAddress()?;

    let (pnp_instance_id, btc_battery) = pnp_devices_info
        .get(&btc_address)
        .map(|i| (i.instance_id.clone(), i.battery))
        .ok_or_else(|| anyhow!("No matching Bluetooth Classic Device in Pnp device: {btc_name}"))?;

    let btc_status = btc_device.ConnectionStatus()? == BluetoothConnectionStatus::Connected;

    Ok(BluetoothInfo {
        name: btc_name,
        battery: btc_battery,
        status: btc_status,
        address: btc_address,
        r#type: BluetoothType::Classic(pnp_instance_id),
    })
}

fn get_pnp_devices_info() -> Result<HashMap<u64, PnpDeviceInfo>> {
    let mut pnp_devices_info: HashMap<u64, PnpDeviceInfo> = HashMap::new();

    let bt_devices_info = get_pnp_bt_devices()?;

    for bt_device_info in bt_devices_info {
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
                    PnpDevicePropertyValue::String(v) => u64::from_str_radix(&v, 16).ok(),
                    _ => None,
                });

            if let (Some(address), Some(battery)) = (address, battery) {
                pnp_devices_info.insert(
                    address,
                    PnpDeviceInfo {
                        address,
                        battery,
                        instance_id: bt_device_info.device_instance_id,
                    },
                );
            }
        }
    }

    Ok(pnp_devices_info)
}

pub fn get_pnp_device_info(device_instance_id: &str) -> Result<PnpDeviceInfo> {
    let bt_device_info = get_pnp_bt_device(device_instance_id)?;

    if let Some(mut props) = bt_device_info.device_instance_properties {
        let battery =
            props
                .remove(&DEVPKEY_Bluetooth_Battery.into())
                .and_then(|value| match value {
                    PnpDevicePropertyValue::Byte(v) => Some(v),
                    _ => None,
                });

        let address = props
            .remove(&DEVPKEY_Bluetooth_DeviceAddress.into())
            .and_then(|value| match value {
                PnpDevicePropertyValue::String(v) => u64::from_str_radix(&v, 16).ok(),
                _ => None,
            });

        if let (Some(address), Some(battery)) = (address, battery) {
            return Ok(PnpDeviceInfo {
                address,
                battery,
                instance_id: bt_device_info.device_instance_id,
            });
        }
    }

    Err(anyhow!(
        "Failed to get address or battery for device instance ID: {device_instance_id}"
    ))
}

fn get_pnp_bt_devices() -> Result<Vec<PnpDeviceNodeInfo>> {
    PnpEnumerator::enumerate_present_devices_and_filter_device_instance_id_by_device_setup_class(
        GUID_DEVCLASS_SYSTEM,
        DeviceInstanceIdFilter::Contains(BT_INSTANCE_ID.to_owned()),
    )
    .map_err(|e| anyhow!("Failed to enumerate pnp devices - {e:?}"))
}

fn get_pnp_bt_device(device_instance_id: &str) -> Result<PnpDeviceNodeInfo> {
    PnpEnumerator::enumerate_present_devices_and_filter_device_instance_id_by_device_setup_class(
        GUID_DEVCLASS_SYSTEM,
        DeviceInstanceIdFilter::Eq(device_instance_id.to_owned()),
    )
    .map_err(|e| {
        anyhow!("Failed to enumerate the instance ID ({device_instance_id}) device - {e:?}")
    })
    .and_then(|d| {
        d.into_iter().next().ok_or_else(|| {
            anyhow!("No Pnp device found for the instance ID ({device_instance_id})")
        })
    })
}
