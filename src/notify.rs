use tauri_winrt_notification::*;

// HKEY_CLASSES_ROOT\AppUserModelId\Windows.SystemToast.BthQuickPair
const BLUETOOTH_APP_ID: &str = "Windows.SystemToast.BthQuickPair";

pub fn notify(title: impl AsRef<str>, text: impl AsRef<str>, mute: bool) {
    Toast::new(BLUETOOTH_APP_ID)
        .title(title.as_ref())
        .text1(text.as_ref())
        .sound((!mute).then_some(Sound::Default))
        .duration(Duration::Short)
        .show()
        .expect("Failied to send notification");
}

pub fn app_notify(text: impl AsRef<str>) {
    Toast::new(BLUETOOTH_APP_ID)
        .title("BlueGauge")
        .text1(text.as_ref())
        .sound(Some(Sound::Default))
        .duration(Duration::Short)
        .show()
        .expect("Failied to send notification");
}
