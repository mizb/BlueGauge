use anyhow::{Result, anyhow};
use tokio::sync::mpsc;
use windows::{
    Devices::{
        Bluetooth::{
            BluetoothConnectionStatus, BluetoothDevice, BluetoothLEDevice,
            GenericAttributeProfile::{
                GattCharacteristic, GattCharacteristicProperties, GattCharacteristicUuids,
                GattClientCharacteristicConfigurationDescriptorValue, GattCommunicationStatus,
                GattServiceUuids, GattValueChangedEventArgs,
            },
        },
        Enumeration::{
            DeviceInformation, DeviceInformationUpdate, DeviceWatcher, DeviceWatcherStatus,
        },
    },
    Foundation::TypedEventHandler,
    Storage::Streams::DataReader,
    core::{GUID, HSTRING, Ref},
};

#[derive(Debug)]
pub enum BluetoothLEDeviceUpdate {
    BatteryLevel(u8),
    ConnectionStatus(bool),
}

#[tokio::test]
async fn watch_ble_device() -> Result<()> {
    let ble_device = BluetoothLEDevice::FromBluetoothAddressAsync(242976932086723)?.get()?;

    // 0000180F-0000-1000-8000-00805F9B34FB
    let battery_services_uuid: GUID = GattServiceUuids::Battery()?;
    // 00002A19-0000-1000-8000-00805F9B34FB
    let battery_level_uuid: GUID = GattCharacteristicUuids::BatteryLevel()?;

    let battery_gatt_services = ble_device
        .GetGattServicesForUuidAsync(battery_services_uuid)?
        .GetResults()?
        .Services()
        .map_err(|e| anyhow!("Failed to get BLE Battery Gatt Services: {e}"))?;

    let battery_gatt_service = battery_gatt_services
        .into_iter()
        .next()
        .ok_or(anyhow!("Failed to get BLE Battery Gatt Service"))?; // 手机蓝牙无电量服务;

    let battery_gatt_chars = battery_gatt_service
        .GetCharacteristicsForUuidAsync(battery_level_uuid)?
        .get()?
        .Characteristics()
        .map_err(|e| anyhow!("Failed to get BLE Battery Gatt Characteristics: {e}"))?;

    let battery_gatt_char = battery_gatt_chars
        .into_iter()
        .next()
        .ok_or_else(|| anyhow!("Failed to get BLE Battery Gatt Characteristic"))?;

    if battery_gatt_char.Uuid()? != battery_level_uuid {
        return Err(anyhow!("Battery level characteristic not found"));
    }

    let properties = battery_gatt_char.CharacteristicProperties()?;

    if !properties.contains(GattCharacteristicProperties::Notify) {
        return Err(anyhow!("Battery level does not support notifications"));
    }

    let (tx, mut rx) = tokio::sync::mpsc::channel(10);

    let tx_status = tx.clone();
    let connection_status_token = {
        let handler = TypedEventHandler::new(
            move |sender: windows::core::Ref<BluetoothLEDevice>, _args| {
                if let Some(ble) = sender.as_ref() {
                    let status = ble.ConnectionStatus()? == BluetoothConnectionStatus::Connected;
                    let _ = tx_status.try_send(BluetoothLEDeviceUpdate::ConnectionStatus(status));
                }
                Ok(())
            },
        );
        ble_device.ConnectionStatusChanged(&handler)?
    };

    let tx_battery = tx.clone();
    let battery_token = {
        let handler = TypedEventHandler::new(
            move |_: windows::core::Ref<GattCharacteristic>,
                  args: windows::core::Ref<GattValueChangedEventArgs>| {
                if let Ok(args) = args.ok() {
                    let value = args.CharacteristicValue()?;
                    let reader = DataReader::FromBuffer(&value)?;
                    let battery = reader.ReadByte()?;
                    let _ = tx_battery.try_send(BluetoothLEDeviceUpdate::BatteryLevel(battery));
                }
                Ok(())
            },
        );
        battery_gatt_char.ValueChanged(&handler)?
    };

    use scopeguard::defer;
    defer! {
        let _ = ble_device.RemoveConnectionStatusChanged(connection_status_token);
        let _ = battery_gatt_char.RemoveValueChanged(battery_token);
    }

    let status = battery_gatt_char
        .WriteClientCharacteristicConfigurationDescriptorAsync(
            GattClientCharacteristicConfigurationDescriptorValue::Notify,
        )?
        .get()?;

    if status != GattCommunicationStatus::Success {
        println!("Failed to subscribe to notifications");
    }

    if let Some(test) = rx.recv().await {
        println!("{test:?}");
        return Ok(());
    }

    Ok(())
}

#[tokio::test]
async fn watch() -> Result<()> {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let watcher = BluetoothWatcher::new(tx.clone())?;

    println!("开始监听蓝牙设备...");
    println!("按 Ctrl+C 退出");

    watcher.start()?;

    while let Some(event) = rx.recv().await {
        match event {
            DeviceEvent::Added(device) => {
                let name = device.Name()?.to_string_lossy();
                let id = device.Id()?.to_string_lossy();
                println!("[+] 设备添加: {} ({})", name, id);
            }
            DeviceEvent::Removed(update) => {
                let id = update.Id()?.to_string_lossy();
                println!("[-] 设备移除: {}", id);
            }
            DeviceEvent::Updated(update) => {
                let id = update.Id()?.to_string_lossy();
                println!("[~] 设备更新: {}", id);
            }
            DeviceEvent::Stopped(status) => {
                println!("[!] 监视器停止: {:?}", status);
            }
        }
    }

    Ok(())
}

enum DeviceEvent {
    Added(DeviceInformation),
    Removed(DeviceInformationUpdate),
    Updated(DeviceInformationUpdate),
    // EnumerationCompleted,
    Stopped(DeviceWatcherStatus),
}

#[derive(Debug, Clone)]
pub struct BluetoothWatcher {
    watcher: DeviceWatcher,
}

impl Drop for BluetoothWatcher {
    fn drop(&mut self) {
        if let Err(e) = self.stop() {
            println!("Failed to stop device watcher: {e}");
        };
    }
}

impl BluetoothWatcher {
    fn new(tx: mpsc::UnboundedSender<DeviceEvent>) -> Result<Self> {
        let ble_selector = BluetoothLEDevice::GetDeviceSelector()?;
        let classic_selector = BluetoothDevice::GetDeviceSelector()?;

        let aqs_filter = HSTRING::from(format!("({}) OR ({})", ble_selector, classic_selector));

        let watcher = DeviceInformation::CreateWatcherAqsFilter(&aqs_filter)?;

        // 注册设备添加事件
        let added_tx = tx.clone();
        let added_token = watcher.Added(&TypedEventHandler::new(
            move |_watcher: Ref<DeviceWatcher>, device_info: Ref<DeviceInformation>| {
                if let Some(device) = device_info.as_ref() {
                    // let _ = added_tx.send(DeviceEvent::Added(device.clone()));
                    println!("Add {}", device.Name().unwrap())
                }
                Ok(())
            },
        ))?;

        // 注册设备移除事件
        let removed_tx = tx.clone();
        let removed_token = watcher.Removed(&TypedEventHandler::new(
            move |_watcher: Ref<DeviceWatcher>, update: Ref<DeviceInformationUpdate>| {
                if let Some(update) = update.as_ref() {
                    let _ = removed_tx.send(DeviceEvent::Removed(update.clone()));
                    println!("Removed {}", update.Id().unwrap())
                }
                Ok(())
            },
        ))?;

        // 注册设备更新事件
        let updated_tx = tx.clone();
        let updated_token = watcher.Updated(&TypedEventHandler::new(
            move |_watcher: Ref<DeviceWatcher>, update: Ref<DeviceInformationUpdate>| {
                if let Some(update) = update.as_ref() {
                    let _ = updated_tx.send(DeviceEvent::Updated(update.clone()));
                    println!("Updated {}", update.Id().unwrap())
                }
                Ok(())
            },
        ))?;

        // 注册枚举完成事件
        // let completed_tx = tx.clone();
        // let completed_token = watcher.EnumerationCompleted(&TypedEventHandler::new(
        //     move |_, _| {
        //         let _ = completed_tx.send(DeviceEvent::EnumerationCompleted);
        //         Ok(())
        //     },
        // ))?;

        // 注册监视器停止事件
        let stopped_tx = tx;
        let _stopped_token = watcher.Stopped(&TypedEventHandler::new(
            move |watcher: Ref<DeviceWatcher>, _| {
                if let Some(watcher) = watcher.as_ref() {
                    let status = watcher.Status().unwrap_or(DeviceWatcherStatus::Stopped);
                    let _ = stopped_tx.send(DeviceEvent::Stopped(status));
                }
                Ok(())
            },
        ))?;

        // 使用 RAII 结构体管理事件处理程序生命周期
        struct WatcherGuard {
            watcher: DeviceWatcher,
            tokens: [i64; 3],
        }

        impl Drop for WatcherGuard {
            fn drop(&mut self) {
                let _ = self.watcher.RemoveAdded(self.tokens[0]);
                let _ = self.watcher.RemoveRemoved(self.tokens[1]);
                let _ = self.watcher.RemoveUpdated(self.tokens[2]);
            }
        }

        let guard = WatcherGuard {
            watcher: watcher.clone(),
            tokens: [added_token, removed_token, updated_token],
        };

        // 防止 guard 被立即丢弃
        std::mem::forget(guard);

        Ok(BluetoothWatcher { watcher })
    }

    fn stop(&self) -> Result<()> {
        let status = self.watcher.Status()?;

        // https://learn.microsoft.com/en-us/uwp/api/windows.devices.enumeration.devicewatcher?view=winrt-26100
        if matches!(
            status,
            DeviceWatcherStatus::Started
                | DeviceWatcherStatus::Aborted
                | DeviceWatcherStatus::EnumerationCompleted
        ) {
            self.watcher.Stop()?;
        }
        Ok(())
    }

    pub fn start(&self) -> windows::core::Result<()> {
        let status = self.watcher.Status()?;

        if matches!(
            status,
            DeviceWatcherStatus::Created
                | DeviceWatcherStatus::Aborted
                | DeviceWatcherStatus::Stopped
        ) {
            self.watcher.Start()?;
        }
        Ok(())
    }
}
