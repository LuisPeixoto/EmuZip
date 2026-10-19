# Troubleshooting

## The *Environment* card shows "Android SDK not found"

EmuZip looks for the SDK in this order: **Settings → Paths**, `ANDROID_HOME`, `ANDROID_SDK_ROOT`, then the default per-OS locations (`~/Library/Android/sdk` on macOS, `~/Android/Sdk` on Linux, `%LOCALAPPDATA%\Android\Sdk` on Windows). The folder must contain `emulator/` and/or `platform-tools/`.

Fix: install the SDK command-line tools (or Android Studio once), or set the path in **Settings → Paths** and press **Save**.

## A device stays on "Starting…" forever

- Check that hardware acceleration is available (Hypervisor.framework on macOS, Windows Hypervisor Platform on Windows, KVM on Linux — your user must be in the `kvm` group).
- Try **Cold boot** from the device menu: a corrupted snapshot is the most common cause.
- Lower RAM or CPU count in the device's hardware options.
- Run `emuzipctl check <id>` to see which of gRPC, console and adb is failing.

## "adb server unavailable"

EmuZip talks to the ADB server on `127.0.0.1:5037`. Start it with:

```sh
adb start-server
```

and make sure no firewall blocks local connections.

## The screen is black or does not update

- The device may still be booting — wait for the lock screen.
- In **Settings → Emulation**, try a different **GPU mode** (`auto`, `host`, `swiftshader_indirect`, `angle_indirect`).
- Lower the stream **FPS**.

## Network inspector shows "TLS tunnel, not decrypted"

HTTPS is only decrypted for apps that trust the EmuZip CA:

- **Rooted images** (Google APIs): the CA can be installed system-wide.
- **Google Play images** are not rooted: the CA is installed as a *user* certificate, so only apps that trust user CAs (Chrome, debug builds) are decrypted. Apps that pin certificates will never be decrypted.

## Downloads are slow or blocked

Set a **download proxy** or a **repository mirror** in **Settings**.

## Text appears in the wrong language

Choose your language in **Settings → Language** (the change is instant). `System` follows the OS language and falls back to English when it is not supported. You can force one for a single run with `EMUZIP_LANG=<code>`.

## Still stuck?

Open an [issue](https://github.com/LuisPeixoto/EmuZip/issues/new/choose) with your OS, EmuZip version, the Android image and the steps to reproduce. Settings → Diagnostics can export useful information.
