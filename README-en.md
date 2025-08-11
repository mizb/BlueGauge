# BlueGauge
A lightweight tray tool for easily checking the battery level of your Bluetooth devices.

![image](screenshots/app.png)

<h3 align="center"> <a href='./README.md'>简体中文</a> | English</h3>

## Function

- [x] Setting：Bluetooth battery level as tray icon  

    - Use system font (default):  
        1. check the device that needs to display the battery, open tray menu -- `Settings` -- `Open Config`
        2. set font  
        `font_name` = `"System Font Nmae, e.g. Microsoft YaHei UI"`  
        `font_color` = `"Hex color code，e.g. #FFFFFF、#00D26A"` (Default `"FollowSystemTheme"`)  
        `font_size` = `0~255` (Default `64`)   
        3. restart BlueGauge
        4. others: the icon color supports connection color matching, set the icon color to the connection color in `Settings`-`Tray Options` (connected as green, disconnected as red)

        <div align="center">
            <img src="screenshots/battery.png" style="width=90%; display:block; margin:0 auto 10px;" />
            <div style="display:flex; justify-content:space-between; width:100%; margin:0 auto;">
                <img src="screenshots/connect.png" alt="左下图片" style="width:45%; display:block;">
                <img src="screenshots/disconnect.png" alt="右下图片" style="width:45%; display:block;">
            </div>
        </div>

    - Use custom pictures  
        1. create an `assets` folder in the BlueGauge directory
            - Default：add `0.png` to `100.png`   
            - Follow system theme：In the `assets` folder, create the `dark` and `light` folders respectively, and add `0.png` to `100.png` photos respectively
        2. restart BlueGauge  


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