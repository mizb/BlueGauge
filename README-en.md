## Function

- [ ] Setting：Device power as tray icon (font or battery icon)
- [x] Setting：Update interval
- [x] Setting：Auto start
- [x] Setting：Shows unconnected devices
- [x] Setting：Truncate devices Name
- [x] Setting：Changing the device power location
- [x] Setting-notice：Mute notice
- [x] Setting-notice：Low battery notice
- [x] Setting-notice：Notification when reconnecting the device
- [x] Setting-notice：Notification when disconnecting the device
- [x] Setting-notice：Notification when adding a new device
- [x] Setting-notice：Notification when moving a new device

## Known Issues & Suggested Solutions

### 1. Currently, BlueGauge successfully retrieves battery levels from low-energy Bluetooth devices and Plug-and-Play (PnP) devices. However, we are unable to fetch the battery status from devices like AirPods and Xbox controllers, which operate on proprietary communication protocols.

**Solution:**

Welcome contributions from developers who can help us extend support for these devices.


### 2. The character length of tray tooltip is currently limited. When the tooltip text exceeds this limit, it gets truncated, which can result in incomplete device names being displayed. This can cause confusion for users, especially when multiple devices are connected.

**Solution:**

1. Limit Device Name Length: Implement a character limit for device names that ensures they fit within the available space of the tray notification. This may require shortening longer names to prevent truncation.

2. Hide Disconnected Devices: Consider not displaying disconnected devices in the tray notifications. This approach would reduce clutter and ensure that only relevant information is shown, thereby preventing text overflow.


### 3.When BlueGauge updates Bluetooth Information related to Bluetooth devices and sends notifications, if the tray menu is active (open), it can lead to the tray menu freezing. Currently considering a bug in the tray-icon library.

- Temporary Fix: Press `Ctrl + Shift + Esc` to open the `Task Manager`. Search for `BlueGauge.exe`. Select the process and click `End Task` to stop it.