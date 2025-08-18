use crate::{
    UserEvent,
    bluetooth::{
        ble::{BluetoothLEDeviceUpdate, find_ble_device, watch_ble_device},
        btc::{find_btc_device, get_pnp_device_info},
        info::{BluetoothInfo, BluetoothType},
    },
    config::Config,
};

use std::sync::{
    Arc, Mutex, OnceLock,
    atomic::{AtomicBool, Ordering},
};

use anyhow::{Result, anyhow};
use windows::Devices::Bluetooth::BluetoothConnectionStatus;
use winit::event_loop::EventLoopProxy;

pub fn listen_bluetooth_devices_info(config: Arc<Config>, proxy: EventLoopProxy<UserEvent>) {
    std::thread::spawn(move || {
        loop {
            let update_interval = config.get_update_interval();

            let mut need_force_update = false;

            for _ in 0..update_interval {
                std::thread::sleep(std::time::Duration::from_secs(1));
                if config.force_update.swap(false, Ordering::SeqCst) {
                    need_force_update = true;
                    break;
                }
            }

            proxy
                .send_event(UserEvent::UpdateTray(need_force_update))
                .expect("Failed to send UpdateTray Event");
        }
    });
}

struct ThreadManager {
    handle: Option<std::thread::JoinHandle<()>>,
    exit_flag: Option<Arc<AtomicBool>>,
    current_bluetooth_info: Option<BluetoothInfo>,
}

impl ThreadManager {
    fn new() -> Self {
        Self {
            handle: None,
            exit_flag: None,
            current_bluetooth_info: None,
        }
    }

    fn has_matching_threa(&self, bluetooth_info: Option<&BluetoothInfo>) -> bool {
        match (bluetooth_info, &self.current_bluetooth_info) {
            (Some(info), Some(current)) => {
                current.r#type == info.r#type && current.address == info.address
            }
            _ => false,
        }
    }

    // 清理资源
    fn cleanup(&mut self, device_name: &str) -> Result<()> {
        if let (Some(handle), Some(exit_flag)) = (self.handle.take(), self.exit_flag.take()) {
            // 设置退出标志
            exit_flag.store(true, Ordering::Relaxed);

            // 等待线程结束
            match handle.join() {
                Ok(()) => println!("[{device_name}] Thread stopped successfully"),
                Err(_) => {
                    eprintln!("[{device_name}] Thread panicked during cleanup");
                    return Err(anyhow!("Thread panicked"));
                }
            }
        } else {
            println!("No thread to stop");
        }
        Ok(())
    }
}

static THREAD_STATE: OnceLock<Mutex<ThreadManager>> = OnceLock::new();

pub fn listen_bluetooth_device_info(
    bluetooth_device: Option<&BluetoothInfo>,
    create: bool,
    proxy: Option<EventLoopProxy<UserEvent>>,
) -> Result<()> {
    let mut state = THREAD_STATE
        .get_or_init(|| Mutex::new(ThreadManager::new()))
        .lock()
        .unwrap();

    if create {
        // 忽略已有匹配线程
        if state.has_matching_threa(bluetooth_device) {
            println!("Thread for device already running");
            return Ok(());
        }

        if state.handle.is_some() {
            let device_name = state
                .current_bluetooth_info
                .as_ref()
                .map(|i| i.name.clone())
                .unwrap_or_else(|| "Unknown Device".to_string());

            if let Err(e) = state.cleanup(&device_name) {
                eprintln!("Failed to cleanup previous thread: {e}");
            }
        }

        // 创建退出标志
        let exit_flag = Arc::new(AtomicBool::new(false));
        let thread_exit_flag = exit_flag.clone();

        // 克隆需要的数据
        let thread_bluetooth_device = match bluetooth_device.cloned() {
            Some(device) => device,
            None => {
                return Err(anyhow!("Bluetooth device is required when creating thread"));
            }
        };
        let thread_proxy = proxy.clone();

        // 创建新线程
        let handle = std::thread::spawn(move || {
            println!(
                "[{}] Bluetooth monitoring thread started",
                thread_bluetooth_device.name
            );

            let mut current_device_info = thread_bluetooth_device.clone();

            if let Some(mutex) = THREAD_STATE.get()
                && let Ok(mut state) = mutex.lock()
            {
                state.current_bluetooth_info = Some(current_device_info.clone());
            }

            while !thread_exit_flag.load(Ordering::Relaxed) {
                if thread_exit_flag.load(Ordering::Relaxed) {
                    break;
                }

                let processing_result = match current_device_info.r#type {
                    BluetoothType::Classic(ref instance_id, address) => process_classic_device(
                        instance_id,
                        address,
                        &current_device_info,
                        &thread_proxy,
                        &thread_exit_flag,
                    ),
                    BluetoothType::LowEnergy(address) => process_le_device(
                        address,
                        &current_device_info,
                        &thread_proxy,
                        &thread_exit_flag,
                    ),
                };

                // 如果处理成功并返回了更新的信息，则更新全局状态
                match processing_result {
                    Ok(Some(new_info)) => {
                        println!(
                            "[{}] Updating device info: Status: {}, Battery: {}",
                            new_info.name, new_info.status, new_info.battery
                        );
                        current_device_info = new_info.clone();

                        if let Some(mutex) = THREAD_STATE.get()
                            && let Ok(mut state) = mutex.lock()
                        {
                            state.current_bluetooth_info = Some(new_info);
                        }
                    }
                    Err(e) => println!("{e}"),
                    _ => (),
                }

                // 对于 LE 设备，不需要睡眠循环，因为 watch_ble_device 会持续监听
                if let BluetoothType::LowEnergy(_) = current_device_info.r#type {
                    continue;
                };

                // 根据设备状态确定总间隔时间（秒）
                let total_interval_secs = match current_device_info.status {
                    true if current_device_info.battery > 30 => 10, // 连接且电量充足：10秒
                    true if current_device_info.battery <= 30 => 7, // 连接但电量低：7秒
                    false => 5,                                     // 未连接：5秒（快速检测）
                    _ => 10,                                        // 默认：10秒
                };

                // 将总时间转换为检查次数（每次100ms）
                let check_count = (total_interval_secs * 10) as usize; // 100ms * 10 = 1秒

                // 睡眠循环
                for _ in 0..check_count {
                    if thread_exit_flag.load(Ordering::Relaxed) {
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(100));
                }
            }

            println!(
                "[{}] Bluetooth monitoring thread exited",
                thread_bluetooth_device.name
            );
        });

        // 保存状态
        state.handle = Some(handle);
        state.exit_flag = Some(exit_flag);
        state.current_bluetooth_info = bluetooth_device.cloned();
    } else {
        let device_name = state
            .current_bluetooth_info
            .as_ref()
            .map(|i| i.name.clone())
            .unwrap_or_else(|| "Unknown Device".to_string());

        state.cleanup(&device_name)?;
    }

    Ok(())
}

fn process_classic_device(
    instance_id: &str,
    address: u64,
    thread_bluetooth_device: &BluetoothInfo,
    proxy: &Option<EventLoopProxy<UserEvent>>,
    exit_flag: &Arc<AtomicBool>,
) -> Result<Option<BluetoothInfo>> {
    if exit_flag.load(Ordering::Relaxed) {
        return Ok(None);
    }

    let pnp_device_info = get_pnp_device_info(instance_id)?;
    let pnp_device_address = pnp_device_info.address;
    let pnp_device_battery = pnp_device_info.battery;

    let btc_device = find_btc_device(address)?;
    let btc_address_u64 = btc_device
        .BluetoothAddress()
        .map_err(|e| anyhow!("Failed to get btc address - {e}"))?;
    let btc_address_mac = format!("{btc_address_u64:012X}");
    let btc_status = btc_device
        .ConnectionStatus()
        .map(|status| status == BluetoothConnectionStatus::Connected)
        .unwrap_or(false);

    if btc_address_mac == pnp_device_address
        && (thread_bluetooth_device.status != btc_status
            || thread_bluetooth_device.battery != pnp_device_battery)
    {
        let current_blc_info = BluetoothInfo {
            name: thread_bluetooth_device.name.clone(),
            battery: pnp_device_battery,
            status: btc_status,
            address: btc_address_mac,
            r#type: thread_bluetooth_device.r#type.clone(),
        };

        if let Some(proxy) = proxy {
            proxy
                .send_event(UserEvent::UpdateTrayForBluetooth(current_blc_info.clone()))
                .map_err(|e| anyhow!("Failed to send UpdateTrayForBluetooth Event - {e}"))?;
        }

        Ok(Some(current_blc_info))
    } else {
        println!(
            "[{}] No need to update current Bluetooth device",
            thread_bluetooth_device.name
        );
        Ok(None)
    }
}

fn process_le_device(
    address: u64,
    thread_bluetooth_device: &BluetoothInfo,
    proxy: &Option<EventLoopProxy<UserEvent>>,
    exit_flag: &Arc<AtomicBool>,
) -> Result<Option<BluetoothInfo>> {
    if exit_flag.load(Ordering::Relaxed) {
        return Ok(None);
    }

    let ble_device = find_ble_device(address)?;

    let device_update = tokio::runtime::Runtime::new()
        .expect("Failed to create runtime")
        .block_on(watch_ble_device(ble_device, exit_flag));

    match device_update {
        Ok(update) => {
            let thread_bluetooth_device = thread_bluetooth_device.clone();
            // 处理更新
            let updated_info = match update {
                BluetoothLEDeviceUpdate::BatteryLevel(battery) => BluetoothInfo {
                    name: thread_bluetooth_device.name,
                    battery,
                    status: thread_bluetooth_device.status,
                    address: thread_bluetooth_device.address,
                    r#type: thread_bluetooth_device.r#type,
                },
                BluetoothLEDeviceUpdate::ConnectionStatus(status) => BluetoothInfo {
                    name: thread_bluetooth_device.name,
                    battery: thread_bluetooth_device.battery,
                    status,
                    address: thread_bluetooth_device.address,
                    r#type: thread_bluetooth_device.r#type,
                },
            };
            // 发送更新事件
            if let Some(proxy) = proxy {
                proxy.send_event(UserEvent::UpdateTrayForBluetooth(updated_info.clone()))?;
            }

            Ok(Some(updated_info))
        }
        Err(e) => Err(anyhow!("BLE channel closed - {e}")),
    }
}

pub fn stop_bluetooth_monitoring() -> Result<()> {
    if let Some(mutex) = THREAD_STATE.get() {
        let mut state = mutex.lock().unwrap();
        let device_name = state
            .current_bluetooth_info
            .as_ref()
            .map(|i| i.name.clone())
            .unwrap_or_else(|| "Unknown Device".to_string());
        state.cleanup(&device_name)?;
    }
    Ok(())
}
