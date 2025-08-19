use crate::{
    UserEvent,
    bluetooth::{
        ble::process_ble_device,
        btc::{get_pnp_devices_info, process_btc_device},
        info::{BluetoothInfo, BluetoothType},
    },
};

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use anyhow::{Result, anyhow};
use scopeguard::defer;
use tokio::sync::{
    mpsc::{Receiver, Sender},
    watch,
};
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
        Enumeration::{DeviceInformation, DeviceInformationUpdate, DeviceWatcher},
    },
    Foundation::TypedEventHandler,
    Storage::Streams::DataReader,
    core::{GUID, Ref},
};
use winit::event_loop::EventLoopProxy;

enum WatchEvent {
    Add(BluetoothInfo),
    Remove(BluetoothType, u64),
    Update(BluetoothInfo),
}

pub struct WatchBluetoothDeviceInfo {
    ble: HashMap</* address */ u64, BluetoothInfo>,
    btc: HashMap</* address */ u64, BluetoothInfo>,
    tx: Sender<WatchEvent>,
    rx: Receiver<WatchEvent>,
    proxy: EventLoopProxy<UserEvent>,
}

impl WatchBluetoothDeviceInfo {
    pub fn new(
        infos: HashSet<BluetoothInfo>,
        proxy: EventLoopProxy<UserEvent>,
    ) -> WatchBluetoothDeviceInfo {
        let mut ble = HashMap::new();
        let mut btc = HashMap::new();

        for info in infos.into_iter() {
            match info.r#type {
                BluetoothType::LowEnergy => {
                    ble.insert(info.address, info);
                }
                BluetoothType::Classic(..) => {
                    btc.insert(info.address, info);
                }
            }
        }

        let (tx, rx) = tokio::sync::mpsc::channel(10);

        WatchBluetoothDeviceInfo {
            ble,
            btc,
            tx,
            rx,
            proxy,
        }
    }

    fn add(&mut self, info: BluetoothInfo) -> Result<()> {
        match info.r#type {
            BluetoothType::Classic(..) => {
                self.btc.insert(info.address, info.clone());
            }
            BluetoothType::LowEnergy => {
                self.ble.insert(info.address, info.clone());
            }
        }

        // self.proxy
        //     .send_event(UserEvent::AddBluetoothInfo(info.clone()))
        //     .map_err(|e| anyhow!("Failed to send `AddBluetoothInfo` Event - {e}"))?;

        self.proxy
            .send_event(UserEvent::UpdateTrayForBluetooth(info))
            .map_err(|e| anyhow!("Failed to send UpdateBluetoothInfo Event - {e}"))?;

        Ok(())
    }

    fn remove(&mut self, r#type: BluetoothType, address: u64) -> Result<()> {
        let info = match r#type {
            BluetoothType::Classic(..) => self.btc.remove(&address),
            BluetoothType::LowEnergy => self.ble.remove(&address),
        };

        if let Some(info) = info {
            // self.proxy
            //     .send_event(UserEvent::RemoveBluetoothInfo(info.clone()))
            //     .map_err(|e| anyhow!("Failed to send `RemoveBluetoothInfo` Event - {e}"))?;

            self.proxy
                .send_event(UserEvent::UpdateTrayForBluetooth(info))
                .map_err(|e| anyhow!("Failed to send UpdateBluetoothInfo Event - {e}"))?;
        }

        Ok(())
    }

    pub fn update(&mut self, info: BluetoothInfo) -> Result<()> {
        match info.r#type {
            BluetoothType::Classic(..) => {
                self.btc.insert(info.address, info.clone());
            }
            BluetoothType::LowEnergy => {
                self.ble.insert(info.address, info.clone());
            }
        }

        self.proxy
            .send_event(UserEvent::UpdateTrayForBluetooth(info))
            .map_err(|e| anyhow!("Failed to send UpdateBluetoothInfo Event - {e}"))?;

        Ok(())
    }

    pub async fn watch(&mut self) -> Result<()> {
        let vec_watcher = self
            .watch_bt_add_remove()
            .map_err(|e| anyhow!("Failed to watch bluetooth added or removed event: {e}"))?;
        vec_watcher.0.Start()?;
        vec_watcher.1.Start()?;

        let exit_tx = self
            .watch_btc_devices()
            .map_err(|e| anyhow!("Failed to watch BTC Devices: {e}"))?;
        self.watch_ble_devices()
            .map_err(|e| anyhow!("Failed to watch BLE Devices: {e}"))?;

        while let Some(event) = self.rx.recv().await {
            match event {
                WatchEvent::Add(info) => {
                    self.add(info)?;
                    // 使获取pnp设备信息的异步线程循环退出
                    exit_tx.send(true).ok();
                    // 当有新设备时，退出watch，然后重新watch
                    break;
                }
                WatchEvent::Remove(r#type, address) => {
                    self.remove(r#type, address)?;
                    // 使获取pnp设备信息的异步线程循环退出
                    exit_tx.send(true).ok();
                    // 当有设备被移除时，退出watch，然后重新watch
                    break;
                }
                WatchEvent::Update(info) => {
                    println!("watch update {info:?}");
                    self.update(info)?;
                }
            }
        }

        vec_watcher.0.Stop()?;
        vec_watcher.1.Stop()?;
        Ok(())
    }

    fn watch_bt_add_remove(&self) -> Result<((DeviceWatcher, DeviceWatcher))> {
        let tx = self.tx.clone();

        let ble_filter = BluetoothLEDevice::GetDeviceSelector()?;
        let btc_filter = BluetoothDevice::GetDeviceSelector()?;

        let btc_watcher = DeviceInformation::CreateWatcherAqsFilter(&btc_filter)?;
        let ble_watcher = DeviceInformation::CreateWatcherAqsFilter(&ble_filter)?;

        // 注册设备添加事件
        let btc_added_tx = tx.clone();
        let btc_added_token = {
            let btc_map = self.btc.clone();
            let handler = TypedEventHandler::new(
                move |_watcher: Ref<DeviceWatcher>, device_info: Ref<DeviceInformation>| {
                    if let Some(device) = device_info.as_ref() {
                        let btc = BluetoothDevice::FromIdAsync(&device.Id()?)?.get()?;
                        if !btc_map.contains_key(&btc.BluetoothAddress()?) {
                            let pnp_devices_info = {
                                let max_retries = 2;
                                let mut attempts = 0;

                                loop {
                                    match get_pnp_devices_info() {
                                        Ok(info) => break info,
                                        Err(e) => {
                                            attempts += 1;
                                            if attempts >= max_retries {
                                                break HashMap::new();
                                            }
                                            println!(
                                                "Failed to get Bluetooth device information: {e}, try again after 2 seconds... (try {attempts}/{max_retries})"
                                            );
                                            std::thread::sleep(std::time::Duration::from_secs(2));
                                        }
                                    }
                                }
                            };

                            if let Ok(info) = process_btc_device(&btc, &pnp_devices_info) {
                                let _ = btc_added_tx.send(WatchEvent::Add(info));
                                println!("Add {:?}", device.Name())
                            }
                        }
                    }
                    Ok(())
                },
            );
            btc_watcher.Added(&handler)?
        };

        let ble_added_tx = tx.clone();
        let ble_added_token = {
            let ble_map = self.ble.clone();
            let handler = TypedEventHandler::new(
                move |_watcher: Ref<DeviceWatcher>, device_info: Ref<DeviceInformation>| {
                    if let Some(device) = device_info.as_ref() {
                        let ble = BluetoothLEDevice::FromIdAsync(&device.Id()?)?.get()?;
                        if !ble_map.contains_key(&ble.BluetoothAddress()?) {
                            if let Ok(info) = process_ble_device(&ble) {
                                let _ = ble_added_tx.send(WatchEvent::Add(info));
                                println!("Add {}", device.Name().unwrap())
                            };
                        }
                        println!("Add {:?}", device.Name())
                    }
                    Ok(())
                },
            );
            ble_watcher.Added(&handler)?
        };

        // 注册设备移除事件
        let btc_removed_tx = tx.clone();
        let btc_removed_token = {
            let btc_map = self.btc.clone();
            let handler = TypedEventHandler::new(
                move |_watcher: Ref<DeviceWatcher>, update: Ref<DeviceInformationUpdate>| {
                    if let Some(update) = update.as_ref() {
                        let btc = BluetoothDevice::FromIdAsync(&update.Id()?)?.get()?;
                        let address = btc.BluetoothAddress()?;
                        if btc_map.contains_key(&address) {
                            let _ = btc_removed_tx.send(WatchEvent::Remove(
                                BluetoothType::Classic(String::new()),
                                address,
                            ));
                            println!("Removed {:?}", btc.Name())
                        }
                    }
                    Ok(())
                },
            );
            btc_watcher.Removed(&handler)?
        };

        let ble_removed_tx = tx.clone();
        let ble_removed_token = {
            let ble_map = self.ble.clone();
            let handler = TypedEventHandler::new(
                move |_watcher: Ref<DeviceWatcher>, update: Ref<DeviceInformationUpdate>| {
                    if let Some(update) = update.as_ref() {
                        let ble = BluetoothLEDevice::FromIdAsync(&update.Id()?)?.get()?;
                        let address = ble.BluetoothAddress()?;
                        if ble_map.contains_key(&address) {
                            let _ = ble_removed_tx
                                .send(WatchEvent::Remove(BluetoothType::LowEnergy, address));
                            println!("Removed {:?}", ble.Name())
                        }
                    }
                    Ok(())
                },
            );
            ble_watcher.Removed(&handler)?
        };

        // 使用 RAII 结构体管理事件处理程序生命周期
        struct WatcherGuard {
            watcher: DeviceWatcher,
            tokens: [i64; 2],
        }

        impl Drop for WatcherGuard {
            fn drop(&mut self) {
                let _ = self.watcher.RemoveAdded(self.tokens[0]);
                let _ = self.watcher.RemoveRemoved(self.tokens[1]);
            }
        }

        let btc_guard = WatcherGuard {
            watcher: btc_watcher.clone(),
            tokens: [btc_added_token, btc_removed_token],
        };

        let ble_guard = WatcherGuard {
            watcher: ble_watcher.clone(),
            tokens: [ble_added_token, ble_removed_token],
        };

        std::mem::forget(btc_guard);
        std::mem::forget(ble_guard);

        Ok((btc_watcher, ble_watcher))
    }

    fn watch_btc_devices(&self) -> Result<tokio::sync::watch::Sender<bool>> {
        let tx = self.tx.clone();

        let btc_devices = Arc::new(Mutex::new(self.btc.clone()));

        let (exit_tx, exit_rx) = watch::channel(false);

        for (address, info) in self.btc.iter() {
            let btc_device = BluetoothDevice::FromBluetoothAddressAsync(address.clone())?.get()?;
            let tx_status = tx.clone();
            let mut info = info.clone();

            // RAII struct 管理 token
            struct ConnectionGuard<'a> {
                device: &'a BluetoothDevice,
                token: i64,
            }
            impl Drop for ConnectionGuard<'_> {
                fn drop(&mut self) {
                    let _ = self.device.RemoveConnectionStatusChanged(self.token);
                }
            }

            let connection_status_token = {
                let handler = TypedEventHandler::new(
                    move |sender: windows::core::Ref<BluetoothDevice>, _args| {
                        if let Some(btc) = sender.as_ref() {
                            let status =
                                btc.ConnectionStatus()? == BluetoothConnectionStatus::Connected;
                            info.status = status;
                            let _ = tx_status.try_send(WatchEvent::Update(info.to_owned()));
                        }
                        Ok(())
                    },
                );
                btc_device.ConnectionStatusChanged(&handler)?
            };

            let _guard = ConnectionGuard {
                device: &btc_device,
                token: connection_status_token,
            };

            // defer! {
            //     let _ = btc_device.RemoveConnectionStatusChanged(connection_status_token);
            // }
        }

        let btc_devices_loop = btc_devices.clone();
        let tx_loop = tx.clone();
        tokio::spawn(async move {
            let mut exit_rx = exit_rx;
            loop {
                tokio::select! {
                    _ = exit_rx.changed() => {
                        if *exit_rx.borrow() {
                            break; // 收到退出信号
                        }
                    }
                    _ = tokio::time::sleep(tokio::time::Duration::from_secs(10)) => {
                        let pnp_devices = get_pnp_devices_info().unwrap_or_default();
                        for (address, pnp_info) in &pnp_devices {
                            if let Ok(btc_devices) = btc_devices_loop.lock() {
                                if let Some(bt_info) = btc_devices.get(address) {
                                    if bt_info.battery != pnp_info.battery {
                                        let mut new_info = bt_info.clone();
                                        new_info.battery = pnp_info.battery;
                                        let _ = tx_loop.try_send(WatchEvent::Update(new_info));
                                    }
                                }
                            }
                            tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                        }
                    }
                }
            }
        });

        Ok(exit_tx)
    }

    fn watch_ble_devices(&self) -> Result<()> {
        let tx = self.tx.clone();

        let ble_devices = self.ble.clone();

        for (address, info) in ble_devices {
            let ble_device = BluetoothLEDevice::FromBluetoothAddressAsync(address)?.get()?;
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

            let tx_status = tx.clone();
            let connection_status_token = {
                let mut info = info.clone();
                let handler = TypedEventHandler::new(
                    move |sender: windows::core::Ref<BluetoothLEDevice>, _args| {
                        if let Some(ble) = sender.as_ref() {
                            let status =
                                ble.ConnectionStatus()? == BluetoothConnectionStatus::Connected;
                            info.status = status;
                            let _ = tx_status.try_send(WatchEvent::Update(info.to_owned()));
                        }
                        Ok(())
                    },
                );
                ble_device.ConnectionStatusChanged(&handler)?
            };

            let tx_battery = tx.clone();
            let battery_token = {
                let mut info = info.clone();
                let handler = TypedEventHandler::new(
                    move |_, args: windows::core::Ref<GattValueChangedEventArgs>| {
                        if let Ok(args) = args.ok() {
                            let value = args.CharacteristicValue()?;
                            let reader = DataReader::FromBuffer(&value)?;
                            let battery = reader.ReadByte()?;
                            info.battery = battery;
                            let _ = tx_battery.try_send(WatchEvent::Update(info.to_owned()));
                        }
                        Ok(())
                    },
                );
                battery_gatt_char.ValueChanged(&handler)?
            };

            // RAII struct 管理 token
            // struct ConnectionGuard<'a> {
            //     device: &'a BluetoothLEDevice,
            //     token: i64,
            // }
            // impl Drop for ConnectionGuard<'_> {
            //     fn drop(&mut self) {
            //         let _ = self.device.RemoveConnectionStatusChanged(self.token);
            //     }
            // }

            // RAII struct 管理 token
            // struct BatteryCharGuard<'a> {
            //     char: &'a GattCharacteristic,
            //     token: i64,
            // }

            // impl Drop for BatteryCharGuard<'_> {
            //     fn drop(&mut self) {
            //         let _ = self.char.RemoveValueChanged(self.token);
            //     }
            // }

            // let _connection_guard = ConnectionGuard {
            //     device: &ble_device,
            //     token: connection_status_token,
            // };

            // let _battery_char_guard = BatteryCharGuard {
            //     char: &battery_gatt_char,
            //     token: battery_token,
            // };

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
                // let _ = tx.try_send(WatchEvent::Update(BluetoothInfo {
                //     name: info.name.clone(),
                //     battery: info.battery,
                //     status: false,
                //     address: info.address,
                //     r#type: info.r#type,
                // }));
                // continue;
                // return Err(anyhow!("Failed to subscribe to notifications"));
            }
        }

        Ok(())
    }
}
