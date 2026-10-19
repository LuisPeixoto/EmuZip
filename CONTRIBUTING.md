# Contributing to EmuZip

Thank you for your interest in EmuZip! Every contribution counts — bug reports, translations, documentation, ideas and code.

By participating you agree to follow our [Code of Conduct](CODE_OF_CONDUCT.md).

## Ways to contribute

- 🐛 **Report a bug** — open an [issue](https://github.com/LuisPeixoto/EmuZip/issues/new/choose) with the template. Include your OS, EmuZip version and the Android image you used.
- 💡 **Suggest a feature** — describe the problem first, then the solution you imagine.
- 🌍 **Translate** — add or improve a language in a single `.po` file. See [docs/translations.md](docs/translations.md).
- 📚 **Improve the docs** — typos, missing steps and better explanations are always welcome.
- 🧑‍💻 **Write code** — pick an issue labelled `good first issue` or `help wanted`.

## Development setup

```sh
git clone https://github.com/LuisPeixoto/EmuZip.git
cd EmuZip
cargo build
cargo test --workspace
```

System requirements, Linux packages and tips are in [docs/development.md](docs/development.md).

## Workflow

1. **Fork** the repository and create a branch from `main`:

   ```sh
   git switch -c feat/short-description
   ```

   Branch prefixes: `feat/`, `fix/`, `docs/`, `refactor/`, `chore/`, `ci/`, `i18n/`.

2. Make focused changes. Small, self-contained commits are easier to review than one large one.
3. Before pushing, make sure all of these pass:

   ```sh
   cargo fmt --all -- --check
   cargo clippy --workspace --all-targets
   cargo test --workspace
   ```

4. Open a pull request against `main` and fill in the template. Link the issue it closes.

## Commit messages

We follow [Conventional Commits](https://www.conventionalcommits.org/):

```
<type>(<scope>): <short summary in the imperative mood>
```

| Type | Use for |
|---|---|
| `feat` | a new user-facing feature |
| `fix` | a bug fix |
| `docs` | documentation only |
| `i18n` | translations |
| `refactor` | code change that neither fixes a bug nor adds a feature |
| `test` | adding or fixing tests |
| `ci` | GitHub Actions and automation |
| `chore` | tooling, dependencies, housekeeping |

Scopes match the crate names without the prefix: `core`, `adb`, `console`, `grpc`, `repo`, `netcap`, `backend`, `cli`, `app`.

Examples: `feat(app): add battery health selector`, `fix(adb): handle empty logcat lines`, `i18n(app): add Italian translation`.

## Code style

- Format with `rustfmt` (configuration in [`rustfmt.toml`](rustfmt.toml)).
- Keep `cargo clippy` clean for the code you touch.
- Match the style of the surrounding code; keep comments short and useful.
- The UI is written in [Slint](https://slint.dev) — files live in `crates/emuzip-app/ui/`.

## User-facing text

The **source language is English**. Every string shown in the UI must go through the translation system:

- Slint: `@tr("Text")`
- Rust: `tr!("Text")` or `tr!("Hello {}", name)`

Add the new string to the translation catalogs (see [docs/translations.md](docs/translations.md)). Strings without a translation fall back to English.

## Reporting security issues

Please do **not** open a public issue for vulnerabilities. See [SECURITY.md](SECURITY.md).

## License

By contributing you agree that your contributions will be licensed under the [MIT License](LICENSE).
