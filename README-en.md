# BlueGauge
A lightweight tray tool for easily checking the battery level of your Bluetooth devices.

![image](screenshots/app.png)

<h3 align="center"> <a href='./README.md'>简体中文</a> | English</h3>

## Function

- [x] Setting：Bluetooth battery level as tray icon  

    - Use system font (default):  
        1. Check the device that needs to display the battery, open tray menu -- `Settings` -- `Open Config`
        2. Set font  
        `font_name` = `"System Font Nmae, e.g. Microsoft YaHei UI"`  
        `font_color` = `"Hex color code，e.g. #FFFFFF、#00D26A"` (Default `"FollowSystemTheme"`)  
        `font_size` = `0~255` (Default `64`)   
        3. restart BlueGauge

    ![image](screenshots/battery.png)

    - Use custom pictures  
        1. create an `assets` folder in the BlueGauge directory
        2. then add pictures  
            - Default：add `0.png` to `100.png`   
            - Follow system theme：add `0_dark.png` to `100_dark.png`，`0_light.png` to `100_light.png`
        3. restart BlueGauge  


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

### 1. Currently, BlueGauge successfully retrieves battery levels from Bluetooth low-energy devices and Bluetooth Classic devices. However, we are unable to fetch the battery status from devices like AirPods and Xbox controllers, which operate on proprietary communication protocols.

**Solution:**

Welcome contributions from developers who can help us extend support for these devices.


### 2. The character length of tray tooltip is currently limited. When the tooltip text exceeds this limit, it gets truncated, which can result in incomplete device names being displayed. This can cause confusion for users, especially when multiple devices are connected.

**Solution:**

1. Limit Device Name Length: Implement a character limit for device names that ensures they fit within the available space of the tray notification. This may require shortening longer names to prevent truncation.

2. Hide Disconnected Devices: Consider not displaying disconnected devices in the tray notifications. This approach would reduce clutter and ensure that only relevant information is shown, thereby preventing text overflow.

## Other Bluetooth battery display software

 - Supports more devices：[Bluetooth Battery Monitor](https://www.bluetoothgoodies.com/) (**Purchase**)

 - Apple: [MagicPods](https://apps.microsoft.com/detail/9P6SKKFKSHKM) (**Purchase**)

 - Huawei: [OpenFreebuds](https://github.com/melianmiko/OpenFreebuds)

 - Samsung: [Galaxy Buds](https://apps.microsoft.com/detail/9NHTLWTKFZNB)