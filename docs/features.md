# Features guide

A tour of EmuZip, screen by screen.

## Devices

<img src="screenshots/devices.png" alt="Devices screen" width="820" />

Create and manage virtual devices.

- **New device** opens a 4-step wizard with 13 hardware presets (Pixel phones, foldable, tablet, TV, Wear OS…), image selection and hardware options such as RAM, CPUs, GPU and cameras.
- Start, **cold boot**, stop and restart.
- **Edit hardware**, **clone**, **rename**, **factory reset** and **delete**.
- **Repair** AVDs whose folder or `config.ini` is broken.
- Search, state filters (*All / Running / Stopped / Error*) and grid or list view.

## Images

<img src="screenshots/images.png" alt="Images screen" width="820" />

Android system images straight from Google's official repository — no `sdkmanager`, no Java.

- **Catalog** tab lists downloadable images across channels; **Installed** shows what you already have.
- Native download with progress, speed and ETA, **SHA-1 verification**, extraction and cancel.

## Player

<img src="screenshots/player.png" alt="Player" width="820" />

The Android screen is streamed over gRPC into the app.

- Touch, drag, keyboard and scroll.
- Back / Home / Recents, Power, Volume, Rotate.
- Screenshot (PNG) and screen recording (MP4).
- Fullscreen (`Esc` exits) and **Fit window**, which resizes the window to the device.
- The tool rail on the right opens panels that slide in next to the device.

## Tool panels

### Hardware simulation

| | |
|---|---|
| <img src="screenshots/tool-battery.png" alt="Battery" width="300" /> | **Battery** — charge level, charger type, status and health. |
| <img src="screenshots/tool-gps.png" alt="GPS" width="300" /> | **GPS** — set latitude, longitude and altitude, jump to preset cities, or simulate a route. |
| <img src="screenshots/tool-connectivity.png" alt="Connectivity" width="300" /> | **Connectivity** — Wi-Fi, mobile data, airplane mode, network speed, latency and signal strength. |
| <img src="screenshots/tool-phone-sms.png" alt="Phone and SMS" width="300" /> | **Phone & SMS** — incoming calls (answer, hold, resume, busy, end) and text messages. |
| <img src="screenshots/tool-sensors.png" alt="Sensors" width="300" /> | **Sensors** — accelerometer with orientation presets, ambient light, proximity, temperature, humidity and pressure. |
| <img src="screenshots/tool-biometrics.png" alt="Biometrics" width="300" /> | **Biometrics** — touch the virtual fingerprint sensor with enrolled (#1–#10) or unknown fingers. |
| <img src="screenshots/tool-cameras.png" alt="Cameras" width="300" /> | **Cameras** — choose the source of each camera: virtual 3D scene, emulated or host webcam. Applies on next boot. |

### System and apps

| | |
|---|---|
| <img src="screenshots/tool-display.png" alt="Display" width="300" /> | **Display** — brightness, Android dark mode, show taps, font scale and density (DPI). |
| <img src="screenshots/tool-identifiers.png" alt="Identifiers" width="300" /> | **Identifiers** — Android ID, phone number and system properties. |
| <img src="screenshots/tool-apps.png" alt="Apps" width="300" /> | **Apps** — install APKs, open deep links, list apps, open / stop / clear data / uninstall. |
| <img src="screenshots/tool-files.png" alt="Files and clipboard" width="300" /> | **Files & clipboard** — push files to the device with progress; share text in both directions. |
| <img src="screenshots/tool-snapshots.png" alt="Snapshots" width="300" /> | **Snapshots** — save the complete device state and return to it in seconds. |
| <img src="screenshots/tool-proxy-language.png" alt="Proxy and language" width="300" /> | **Proxy & language** — global Android HTTP proxy (`10.0.2.2` is your computer) and system locale. |

### Developer tools

| | |
|---|---|
| <img src="screenshots/tool-shell.png" alt="Shell" width="300" /> | **Shell** — run `adb shell` commands, with one-click presets. |
| <img src="screenshots/tool-logcat.png" alt="Logcat" width="300" /> | **Logcat** — live stream (last 800 lines) with level and text filters, pause, clear and export. |
| <img src="screenshots/tool-performance.png" alt="Performance" width="300" /> | **Performance** — emulator process CPU/RAM on the host, plus Android CPU, RAM and GPU frame times. |
| <img src="screenshots/tool-performance-processes.png" alt="Top consumers" width="300" /> | **Top consumers** — heaviest processes by CPU or RAM with *kill*, and a **memory stress** test with an LMK counter. |

## Network inspector

A MITM proxy written in Rust with its own CA.

- Request/response list with headers, body (pretty JSON, image preview, gzip/brotli decoded) and timings.
- Export to **HAR 1.2**, **JSON**, **CSV**, **Postman v2.1** and **cURL**.
- On rooted images the CA is installed system-wide; on non-rooted images only apps that trust user CAs are decrypted.

## Settings

<img src="screenshots/settings.png" alt="Settings" width="820" />

Theme (light / dark / system), **language** (applied instantly), paths with validation, embedded display, FPS, GPU mode, cold boot, audio, new-device defaults, proxy, repository mirror and diagnostics export.
