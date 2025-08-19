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
    Arc,
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

            let _ = proxy.send_event(UserEvent::UpdateTray(need_force_update));
        }
    });
}

pub struct Watcher {
    handle: Option<std::thread::JoinHandle<()>>,
    exit_flag: Arc<AtomicBool>,
    device_name: String,
}

impl Watcher {
    pub fn start(device: BluetoothInfo, proxy: EventLoopProxy<UserEvent>) -> Result<Self> {
        println!("[{}]: Starting the watch thread...", device.name);
        let exit_flag = Arc::new(AtomicBool::new(false));
        let thread_exit_flag = exit_flag.clone();
        let device_name = device.name.clone();

        let handle = std::thread::spawn(move || {
            watch_loop(device, proxy, thread_exit_flag);
        });

        Ok(Self {
            handle: Some(handle),
            exit_flag,
            device_name,
        })
    }

    pub fn stop(mut self) -> Result<()> {
        println!("[{}]: Stopping the watch thread...", self.device_name);
        if let (Some(handle), exit_flag) = (self.handle.take(), &self.exit_flag) {
            exit_flag.store(true, Ordering::Relaxed);

            if let Err(_) = handle.join() {
                return Err(anyhow!(
                    "[{}]: Panic occurs during thread cleaning",
                    self.device_name
                ));
            }
            println!("[{}]: The watch thread has been stopped.", self.device_name);
        }
        Ok(())
    }
}

fn watch_loop(
    initial_device_info: BluetoothInfo,
    proxy: EventLoopProxy<UserEvent>,
    exit_flag: Arc<AtomicBool>,
) {
    println!(
        "[{}]: The watch thread is started。",
        initial_device_info.name
    );
    let mut current_device_info = initial_device_info;

    // 如果是 BLE 设备，则只创建一次 Tokio 运行时
    let runtime = if matches!(current_device_info.r#type, BluetoothType::LowEnergy) {
        Some(tokio::runtime::Runtime::new().expect("Failed to create a Tokio runtime"))
    } else {
        None
    };

    while !exit_flag.load(Ordering::Relaxed) {
        let processing_result = match &current_device_info.r#type {
            BluetoothType::Classic(instance_id) => {
                process_classic_device(instance_id, &current_device_info, &proxy)
            }
            BluetoothType::LowEnergy => {
                // 复用已创建的运行时
                let rt = runtime.as_ref().unwrap();
                process_le_device(&current_device_info, &proxy, &exit_flag, rt)
            }
        };

        match processing_result {
            Ok(Some(new_info)) => {
                println!(
                    "[{}]: Status -> {}, Battery -> {}",
                    new_info.name, new_info.status, new_info.battery
                );
                current_device_info = new_info;
            }
            Err(e) => {
                eprintln!(
                    "[{}]: Failed to process device - {e}",
                    current_device_info.name
                );
                break; // 遇到严重错误时退出循环
            }
            _ => (), // 没有更新，继续循环
        }

        // 对于经典蓝牙设备，使用简单的休眠。循环条件已经检查了退出标志。
        if let BluetoothType::Classic(_) = current_device_info.r#type {
            let sleep_duration = match current_device_info {
                _ if !current_device_info.status => std::time::Duration::from_secs(5), // 未连接
                _ if current_device_info.battery <= 30 => std::time::Duration::from_secs(7), // 低电量
                _ => std::time::Duration::from_secs(10), // 已连接且电量充足
            };
            std::thread::sleep(sleep_duration);
        }
        // 对于 BLE 设备, `watch_ble_device` 函数会自己处理等待，可立即进入下一次循环。
    }

    println!(
        "[{}]: The watch thread has exited.",
        current_device_info.name
    );
}

fn process_classic_device(
    instance_id: &str,
    current_device_info: &BluetoothInfo,
    proxy: &EventLoopProxy<UserEvent>,
) -> Result<Option<BluetoothInfo>> {
    let pnp_info = get_pnp_device_info(instance_id)?;
    let btc_device = find_btc_device(current_device_info.address)?;

    let btc_status = btc_device.ConnectionStatus()? == BluetoothConnectionStatus::Connected;

    // 检查是否有必要更新
    if current_device_info.status != btc_status
        || current_device_info.battery != pnp_info.battery
        && current_device_info.address == pnp_info.address
    {
        let new_info = BluetoothInfo {
            status: btc_status,
            battery: pnp_info.battery,
            ..current_device_info.clone()
        };

        let _ = proxy.send_event(UserEvent::UpdateTrayForBluetooth(new_info.clone()));
        Ok(Some(new_info))
    } else {
        Ok(None) // 没有变化
    }
}

fn process_le_device(
    current_device_info: &BluetoothInfo,
    proxy: &EventLoopProxy<UserEvent>,
    exit_flag: &Arc<AtomicBool>,
    runtime: &tokio::runtime::Runtime, // 将运行时传入
) -> Result<Option<BluetoothInfo>> {
    let ble_device = find_ble_device(current_device_info.address)?;

    // 异步函数现在会处理更新
    match runtime.block_on(watch_ble_device(ble_device, exit_flag)) {
        Ok(update) => {
            let mut new_info = current_device_info.clone();
            match update {
                BluetoothLEDeviceUpdate::BatteryLevel(battery) => new_info.battery = battery,
                BluetoothLEDeviceUpdate::ConnectionStatus(status) => new_info.status = status,
            };

            let _ = proxy.send_event(UserEvent::UpdateTrayForBluetooth(new_info.clone()));
            Ok(Some(new_info))
        }
        Err(e) => Err(anyhow!("BLE device watch failed: {e}")),
    }
}
