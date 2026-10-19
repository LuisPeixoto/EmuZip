# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.3.0] - 2026-10-19

First public release as an open-source project.

### Added

- **Devices**: 4-step creation wizard with 13 hardware profiles; start, cold boot, stop, restart, edit hardware, clone, rename, factory reset, delete and repair of broken AVDs; search, filters and grid/list views.
- **Images**: official Google system-image catalog (8 channels) with native download, progress, speed, ETA, SHA-1 verification and cancellation — no `sdkmanager` or Java.
- **Player** with the Android screen embedded in the app through gRPC streaming: touch, drag, keyboard, scroll, back/home/recents, power, volume and rotation.
- **Tool panels**: battery, GPS (places and simulated route), connectivity, phone and SMS, sensors, biometrics, cameras, display, identifiers, apps, files, clipboard, snapshots, logcat, shell, and proxy/locale.
- **Captures**: screenshots (PNG) and screen recordings (MP4).
- **Network inspector**: MITM proxy with its own CA and export to HAR 1.2, JSON, CSV, Postman v2.1 and cURL.
- **Performance** monitor with host and guest CPU, RAM and GPU, 60 s charts, top consumers and memory stress.
- **Activity center** with real progress for long-running tasks.
- **Internationalization**: English (default), Português (Brasil), Español, Français, Deutsch and 中文 (简体), with live switching in Settings.
- **`emuzipctl`** command-line tool.
- Release workflow producing `.dmg`, NSIS installer, `.deb` and `.AppImage`.

[Unreleased]: https://github.com/LuisPeixoto/EmuZip/compare/v0.3.0...HEAD
[0.3.0]: https://github.com/LuisPeixoto/EmuZip/releases/tag/v0.3.0
