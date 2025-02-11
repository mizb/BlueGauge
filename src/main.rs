#![allow(non_snake_case)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod bluetooth;
mod systray;
mod config;
mod notify;
mod language;
mod startup;

use crate::{systray::show_systray, notify::notify};

#[tokio::main]
async fn main() {
    if let Err(err) = show_systray().await {
        notify("BlueGauge", &err.to_string(), false).expect("Failed to show toast notification");
    }
}
