# Command-line interface — `emuzipctl`

`emuzipctl` exposes the same engine as the desktop app, which makes EmuZip easy to script and to use on CI machines.

Build it with `cargo build --release -p emuzip-cli`; the binary is `target/release/emuzipctl`.

```
emuzipctl <command>
  list                              devices
  images                            installed images
  catalog                           images available for download
  download <package|id>             download and install an image
  profiles                          hardware profiles
  create <name> <image> [profile]   create an AVD (no Java needed)
  start <id> [--cold] [--window]    start (headless unless --window)
  stop <id> | delete <id> | wipe <id> | repair <id>
  clone <id> <new-name>
  install <id> <apk>
  push <id> <file> [destination]
  screenshot <id> <out.png>         via gRPC
  check <id>                        check gRPC, console and adb on a running device
```

## Examples

List what can be downloaded and install an image:

```sh
emuzipctl catalog
emuzipctl download <package-or-id>
emuzipctl images
```

Create and start a device:

```sh
emuzipctl profiles
emuzipctl create "Pixel 9" <image-id> pixel_9
emuzipctl list                 # note the device id
emuzipctl start <id>           # headless
emuzipctl start <id> --window  # with the emulator window
```

Install an app, push a file, take a screenshot:

```sh
emuzipctl install <id> app-debug.apk
emuzipctl push <id> ./data.json /sdcard/Download/
emuzipctl screenshot <id> screen.png
```

Diagnose a running device:

```sh
emuzipctl check <id>
```

## Environment

| Variable | Effect |
|---|---|
| `ANDROID_HOME`, `ANDROID_SDK_ROOT` | Android SDK location |
| `ANDROID_AVD_HOME` | AVD folder (default `~/.android/avd`) |
| `EMUZIP_LANG` | Force the UI language of the desktop app (`en`, `pt-BR`, `es`, `fr`, `de`, `zh-CN`) |
