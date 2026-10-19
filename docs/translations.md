# Translations

EmuZip's interface is written in **English** and translated with standard [gettext](https://www.gnu.org/software/gettext/) `.po` files. No code changes are needed to *improve* an existing language, and adding a new one takes a few lines.

## Available languages

| Language | Folder | Code |
|---|---|---|
| English (source) | — | `en` |
| Português (Brasil) | `pt_BR` | `pt-BR` |
| Español | `es` | `es` |
| Français | `fr` | `fr` |
| Deutsch | `de` | `de` |
| 中文 (简体) | `zh_CN` | `zh-CN` |

Catalogs live in `crates/emuzip-app/translations/<folder>/LC_MESSAGES/emuzip-app.po`.

## Improve an existing translation

1. Open the `.po` file of your language.
2. Edit the `msgstr` of the entry you want to fix. **Never edit `msgid`** — it is the English text used as the key.
3. Keep placeholders exactly as they are: `{}` is replaced by a value at runtime (`"{} installed"` → `"{} instalado"`).
4. Test it:

   ```sh
   EMUZIP_LANG=pt-BR cargo run -p emuzip-app
   ```

## Add a new language

1. **Copy the template** and fill it in:

   ```sh
   mkdir -p crates/emuzip-app/translations/it/LC_MESSAGES
   cp crates/emuzip-app/i18n/emuzip-app.pot \
      crates/emuzip-app/translations/it/LC_MESSAGES/emuzip-app.po
   ```

   Set the header (`Language: it`, `X-Language-Name: Italiano`) and write every `msgstr`. Untranslated entries (empty `msgstr`) fall back to English.

2. **Register it** in `crates/emuzip-app/src/i18n.rs`:

   ```rust
   // (saved setting, .po folder, display name)
   pub const LANGUAGES: [(&str, &str, &str); 8] = [
       // …
       ("it", "it", "Italiano"),
   ];

   const CATALOGS: [(&str, &str); 6] = [
       // …
       ("it", include_str!("../translations/it/LC_MESSAGES/emuzip-app.po")),
   ];
   ```

   and map the system locale in `resolve()`:

   ```rust
   "it" => "it",
   ```

3. Validate the catalogs (CI runs the same check):

   ```sh
   python3 .github/scripts/check_translations.py
   ```

4. Run the app, open **Settings → Language** and check your language end to end.
5. Open a pull request titled `i18n(app): add Italian translation`.

## Context entries

Some English words need different translations depending on where they appear (for example *All* in the device filter vs. the image filter, which differ by grammatical gender in Portuguese). These entries have a `msgctxt`:

```po
msgctxt "devices"
msgid "All"
msgstr "Todos"

msgctxt "images"
msgid "All"
msgstr "Todas"
```

In the code they are written as `@tr("devices" => "All")` (Slint) and `tr!("devices" => "All")` (Rust). If your language does not need the distinction, simply give both entries the same `msgstr`.

## Adding new text in code

- Slint: `@tr("Text")`
- Rust: `tr!("Text")`, or `tr!("Starting {}", name)` with placeholders
- With context: `@tr("ctx" => "Text")` / `tr!("ctx" => "Text")`

Then add the new `msgid` to the `.pot` template and to every catalog.
