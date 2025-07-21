# BlueGauge
A lightweight tray tool for easily checking the battery level of your Bluetooth devices.

![image](https://raw.githubusercontent.com/iKineticate/BlueGauge/main/screenshots/app.png)

<h3 align="center"> <a href='./README.md'>简体中文</a> | English</h3>

## Function

- [ ] Setting：Bluetooth Device power as tray icon
- [x] Setting：Auto start
- [x] Setting：Update interval
- [x] Setting-tooltip：Shows unconnected devices
- [x] Setting-tooltip：Truncate devices Name
- [x] Setting-tooltip：Changing the device power location
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
