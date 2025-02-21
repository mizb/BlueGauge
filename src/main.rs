#![allow(non_snake_case)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod bluetooth;
mod config;
mod language;
mod notify;
mod startup;
mod systray;

use crate::{notify::notify, systray::show_systray};

#[tokio::main]
async fn main() {
    if let Err(err) = show_systray().await {
        notify("BlueGauge", &err.to_string(), false).expect("Failed to show toast notification");
    }
}
