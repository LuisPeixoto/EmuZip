# Development guide

## Prerequisites

- **Rust** (stable) — install with [rustup](https://rustup.rs). Also install the formatter and linter:

  ```sh
  rustup component add rustfmt clippy
  ```

- **Android SDK** with `emulator` and `platform-tools`, only if you want to run devices while developing. Building and unit tests do not need it.

### Linux packages

```sh
sudo apt-get install -y libfontconfig1-dev libxkbcommon-dev libwayland-dev \
  libgtk-3-dev libxcb-shape0-dev libxcb-xfixes0-dev
```

### macOS / Windows

No extra system packages are needed.

## Build, run, test

```sh
cargo build                          # debug build of the whole workspace
cargo run -p emuzip-app              # run the desktop app
cargo run -p emuzip-cli -- list      # run emuzipctl
cargo test --workspace               # unit tests
```

Release builds:

```sh
cargo build --release                # target/release/EmuZip and emuzipctl
```

## Quality checks

These are exactly what CI runs:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets
cargo test --workspace
```

Fix formatting with `cargo fmt --all` (configuration in [`rustfmt.toml`](../rustfmt.toml)).

## Packaging

| Platform | How |
|---|---|
| macOS | `./dist/package-macos.sh` → `dist/EmuZip.app` and `dist/EmuZip.dmg` |
| All | `cargo install cargo-packager --locked && cargo packager --release -p emuzip-app` → `.dmg`, NSIS `.exe`, `.deb`, `.AppImage` |

Releases are built automatically by the **Release** workflow when you push a tag like `v0.4.0`.

## Working on the UI

The UI is in [Slint](https://slint.dev) (`crates/emuzip-app/ui/*.slint`), compiled by `build.rs`. A few tips:

- Edit a `.slint` file and re-run `cargo run -p emuzip-app`; the compiler reports errors with file and line.
- All user-facing text must use `@tr("…")` (Slint) or `tr!("…")` (Rust). See [translations.md](translations.md).
- Try other languages without changing settings: `EMUZIP_LANG=pt-BR cargo run -p emuzip-app`.
- The `Api` global in `ui/store.slint` declares callbacks that the Rust modules in `src/` implement.

## Releasing

1. Update the version in the workspace `Cargo.toml` and in `dist/package-macos.sh`.
2. Move the *Unreleased* notes in [`CHANGELOG.md`](../CHANGELOG.md) under the new version.
3. Commit, then tag and push:

   ```sh
   git tag -a v0.4.0 -m "EmuZip 0.4.0"
   git push origin main --tags
   ```

4. The **Release** workflow builds the installers and attaches them to a GitHub release.
