<div align="center">

<img src="docs/assets/logo.png" alt="EmuZip logo" width="112" />

# EmuZip

**A fast, lightweight and beautiful Android emulator manager — written in Rust.**

Create, run and debug Android devices with the screen embedded right inside the app.
No Android Studio. No Java. No `sdkmanager`.

[![CI](https://github.com/LuisPeixoto/EmuZip/actions/workflows/ci.yml/badge.svg)](https://github.com/LuisPeixoto/EmuZip/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/LuisPeixoto/EmuZip?include_prereleases&color=5b5bf0)](https://github.com/LuisPeixoto/EmuZip/releases)
[![License: MIT](https://img.shields.io/badge/license-MIT-5b5bf0.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-stable-orange.svg?logo=rust)](https://www.rust-lang.org)
[![UI: Slint](https://img.shields.io/badge/UI-Slint-2379F4.svg)](https://slint.dev)
[![PRs welcome](https://img.shields.io/badge/PRs-welcome-brightgreen.svg)](CONTRIBUTING.md)

[English](README.md) · [Português (Brasil)](README.pt-BR.md)

<br />

<img src="docs/screenshots/tool-gps.png" alt="EmuZip running Android with the GPS panel open" width="900" />

</div>

---

## Why EmuZip?

The official Android tooling is powerful, but heavy. EmuZip drives the **official Google Android Emulator** (or plain QEMU) through a small native app that starts instantly, and gives you every tool you reach for while developing — in one window.

- ⚡ **Native and light** — a single Rust binary with a [Slint](https://slint.dev) UI. No JVM, no Electron.
- 🖥️ **Embedded display** — the Android screen is streamed over gRPC straight into the app, with touch, keyboard, scroll and rotation.
- 📦 **Official images, zero setup** — browse Google's system-image catalog, download with live progress and SHA-1 verification.
- 🧰 **Everything in a side panel** — battery, GPS, network, sensors, camera, biometrics, logcat, shell, performance and more.
- 🌍 **Speaks your language** — English by default, plus Português (BR), Español, Français, Deutsch and 中文, switchable live.
- 🔎 **Built-in network inspector** — a MITM proxy written in Rust that exports HAR, Postman, cURL, JSON and CSV.

## Screenshots

<table>
  <tr>
    <td width="50%"><img src="docs/screenshots/devices.png" alt="Devices" /><br /><sub><b>Devices</b> — create, start, clone and manage virtual devices.</sub></td>
    <td width="50%"><img src="docs/screenshots/images.png" alt="Android images" /><br /><sub><b>Images</b> — official system images, verified and installed natively.</sub></td>
  </tr>
  <tr>
    <td width="50%"><img src="docs/screenshots/player.png" alt="Immersive player" /><br /><sub><b>Player</b> — immersive device view with a slim tool rail.</sub></td>
    <td width="50%"><img src="docs/screenshots/settings.png" alt="Settings" /><br /><sub><b>Settings</b> — theme, language, paths and emulation options.</sub></td>
  </tr>
</table>

<details>
<summary><b>Tool panels</b> — click to expand the full gallery</summary>

<br />

<table>
  <tr>
    <td width="50%"><img src="docs/screenshots/tool-battery.png" alt="Battery" /><br /><sub><b>Battery</b> — level, charger, status and health.</sub></td>
    <td width="50%"><img src="docs/screenshots/tool-connectivity.png" alt="Connectivity" /><br /><sub><b>Connectivity</b> — Wi-Fi, data, speed, latency and signal.</sub></td>
  </tr>
  <tr>
    <td width="50%"><img src="docs/screenshots/tool-phone-sms.png" alt="Phone and SMS" /><br /><sub><b>Phone &amp; SMS</b> — simulate calls and messages.</sub></td>
    <td width="50%"><img src="docs/screenshots/tool-sensors.png" alt="Sensors" /><br /><sub><b>Sensors</b> — accelerometer, light, proximity and more.</sub></td>
  </tr>
  <tr>
    <td width="50%"><img src="docs/screenshots/tool-biometrics.png" alt="Biometrics" /><br /><sub><b>Biometrics</b> — touch a virtual fingerprint sensor.</sub></td>
    <td width="50%"><img src="docs/screenshots/tool-cameras.png" alt="Cameras" /><br /><sub><b>Cameras</b> — virtual 3D scene, emulated or webcam.</sub></td>
  </tr>
  <tr>
    <td width="50%"><img src="docs/screenshots/tool-display.png" alt="Display" /><br /><sub><b>Display</b> — brightness, dark mode, density and font scale.</sub></td>
    <td width="50%"><img src="docs/screenshots/tool-identifiers.png" alt="Identifiers" /><br /><sub><b>Identifiers</b> — Android ID, phone number, system properties.</sub></td>
  </tr>
  <tr>
    <td width="50%"><img src="docs/screenshots/tool-apps.png" alt="Apps" /><br /><sub><b>Apps</b> — install APKs, open deep links, clear and uninstall.</sub></td>
    <td width="50%"><img src="docs/screenshots/tool-files.png" alt="Files and clipboard" /><br /><sub><b>Files &amp; clipboard</b> — push files, share text both ways.</sub></td>
  </tr>
  <tr>
    <td width="50%"><img src="docs/screenshots/tool-snapshots.png" alt="Snapshots" /><br /><sub><b>Snapshots</b> — save the whole state, restore in seconds.</sub></td>
    <td width="50%"><img src="docs/screenshots/tool-proxy-language.png" alt="Proxy and language" /><br /><sub><b>Proxy &amp; language</b> — global HTTP proxy and system locale.</sub></td>
  </tr>
  <tr>
    <td width="50%"><img src="docs/screenshots/tool-shell.png" alt="Shell" /><br /><sub><b>Shell</b> — run <code>adb shell</code> commands with presets.</sub></td>
    <td width="50%"><img src="docs/screenshots/tool-logcat.png" alt="Logcat" /><br /><sub><b>Logcat</b> — live stream with level and text filters.</sub></td>
  </tr>
  <tr>
    <td width="50%"><img src="docs/screenshots/tool-performance.png" alt="Performance" /><br /><sub><b>Performance</b> — host and Android CPU, RAM and GPU.</sub></td>
    <td width="50%"><img src="docs/screenshots/tool-performance-processes.png" alt="Top consumers" /><br /><sub><b>Top consumers</b> — find heavy processes, stress memory.</sub></td>
  </tr>
</table>

</details>

## Features

| | |
|---|---|
| **Devices** | 4-step creation wizard with 13 hardware profiles · start, cold boot, stop, restart · clone, rename, factory reset, delete · repair broken AVDs · search, filters, grid or list. |
| **Images** | Official Google catalog (8 channels) · native download with progress, speed and ETA · SHA-1 verification · cancel anytime. |
| **Player** | gRPC screen streaming · touch, drag, keyboard and scroll · back / home / recents · power, volume, rotate · fullscreen and *fit window*. |
| **Tools** | Battery · GPS (cities and simulated routes) · network · phone and SMS · sensors · biometrics · cameras · display · identifiers · apps · files · clipboard · snapshots · logcat · shell · proxy and locale. |
| **Captures** | Screenshots (PNG) and screen recordings (MP4). |
| **Network inspector** | Rust MITM proxy with its own CA · request/response viewer (pretty JSON, image preview, gzip/brotli) · export to **HAR 1.2, JSON, CSV, Postman v2.1, cURL**. |
| **Performance** | Host and guest CPU / RAM / GPU · 60 s charts · top consumers with kill · memory stress with LMK counter · CSV export. |
| **Activity center** | Real progress for downloads, device creation, boots and installs. |
| **Languages** | English, Português (BR), Español, Français, Deutsch, 中文 (简体) — live switching. |
| **CLI** | `emuzipctl` for scripting and CI. See [docs/cli.md](docs/cli.md). |

## Installation

### Download

Grab the latest installer for your platform from the [**Releases**](https://github.com/LuisPeixoto/EmuZip/releases) page:

| Platform | Package |
|---|---|
| macOS (Apple Silicon / Intel) | `.dmg` |
| Windows | `.exe` installer (NSIS) |
| Linux | `.deb` or `.AppImage` |

### Requirements

- **Android SDK** with the `emulator` and `platform-tools` packages. EmuZip finds it automatically (`ANDROID_HOME`, `ANDROID_SDK_ROOT`, or the default per-OS locations) — or point to it in **Settings → Paths**.
- Hardware acceleration for the emulator: Hypervisor.framework (macOS), Windows Hypervisor Platform (Windows) or KVM (Linux).
- **QEMU** is optional.

No Java and no `sdkmanager` are required — images are downloaded natively.

### Build from source

```sh
git clone https://github.com/LuisPeixoto/EmuZip.git
cd EmuZip
cargo build --release
```

You get two binaries in `target/release/`: **`EmuZip`** (the app) and **`emuzipctl`** (the CLI). Linux needs a few system libraries — see [docs/development.md](docs/development.md).

Packaging:

```sh
./dist/package-macos.sh      # dist/EmuZip.app + dist/EmuZip.dmg
cargo install cargo-packager --locked
cargo packager --release -p emuzip-app   # dmg / nsis / deb / appimage
```

## Quick start

1. Open EmuZip and go to **Images**. Pick an image from the **Catalog** and download it.
2. Go to **Devices → New device**, choose a hardware profile and the image you just installed.
3. Press **Start**. The Android screen appears inside the app.
4. Use the tool rail on the right to change battery, location, network, sensors and more.

Prefer the terminal?

```sh
emuzipctl catalog                       # list downloadable images
emuzipctl download <package|id>         # download and install one
emuzipctl create "Pixel 9" <image> pixel_9
emuzipctl list                          # shows the device id
emuzipctl start <id> --window
emuzipctl screenshot <id> shot.png
```

## Languages

The interface is **English by default** and follows your system language when you pick **System** in *Settings → Language*. Available: English, Português (Brasil), Español, Français, Deutsch and 中文 (简体).

Want to add your language? It takes one `.po` file — see [docs/translations.md](docs/translations.md).

```sh
EMUZIP_LANG=pt-BR ./target/release/EmuZip   # try a language without touching settings
```

## Project layout

| Crate | Purpose |
|---|---|
| [`emuzip-core`](crates/emuzip-core) | Models, settings (TOML), hardware profiles, installed images |
| [`emuzip-adb`](crates/emuzip-adb) | Pure-Rust ADB protocol (shell, exec, sync push/pull, install, logcat) |
| [`emuzip-console`](crates/emuzip-console) | Emulator console client (network, signal, snapshots, rotation) |
| [`emuzip-grpc`](crates/emuzip-grpc) | Emulator gRPC client (tonic; protos compiled with protox, no `protoc`) |
| [`emuzip-repo`](crates/emuzip-repo) | Image catalog and downloads |
| [`emuzip-netcap`](crates/emuzip-netcap) | MITM proxy, CA, storage and exporters |
| [`emuzip-backend`](crates/emuzip-backend) | AVD (Java-free) and QEMU backends |
| [`emuzip-cli`](crates/emuzip-cli) | `emuzipctl` command-line tool |
| [`emuzip-app`](crates/emuzip-app) | The `EmuZip` desktop app (Slint) |

A deeper tour lives in [docs/architecture.md](docs/architecture.md).

## Documentation

- [Architecture](docs/architecture.md)
- [Features guide](docs/features.md)
- [Command-line interface](docs/cli.md)
- [Translations](docs/translations.md)
- [Development guide](docs/development.md)
- [Troubleshooting](docs/troubleshooting.md)
- [Changelog](CHANGELOG.md)

## Contributing

Contributions of every size are welcome — bug reports, translations, docs and code. Please read [CONTRIBUTING.md](CONTRIBUTING.md) and our [Code of Conduct](CODE_OF_CONDUCT.md) first. Found a security issue? See [SECURITY.md](SECURITY.md).

## License

EmuZip is released under the [MIT License](LICENSE).

Android and the Android robot are trademarks of Google LLC. EmuZip is an independent project and is not affiliated with or endorsed by Google.
