#![allow(non_snake_case)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod bluetooth;
mod systray;
use win_toast_notify::WinToastNotify;
use crate::systray::show_systray;

fn main() {
    if let Err(err) = show_systray() {
        WinToastNotify::new()
            .set_title("BlueGauge")
            .set_messages(vec!["Failed to build the system tray.", &err.message()])
            .show()
            .expect("Failed to show toast notification")
    }
}
