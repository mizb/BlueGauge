use tauri_winrt_notification::*;

// HKEY_CLASSES_ROOT\AppUserModelId\Windows.SystemToast.BthQuickPair
const BLUETOOTH_APP_ID: &str = "Windows.SystemToast.BthQuickPair";

pub fn notify(title: &str, text: &str, mute: bool) {
    Toast::new(BLUETOOTH_APP_ID)
        .title(title)
        .text1(text)
        .sound((!mute).then_some(Sound::Default))
        .duration(Duration::Short)
        .show()
        .expect("unable to send notification");
}
