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
    show_systray()
        .await
        .inspect_err(|e| notify("BlueGauge", &e.to_string(), false))
        .expect("Failed to show systray");
}
