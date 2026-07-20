# Internationalization (i18n)

Perry's i18n system lets you write natural English strings and have them automatically translated at compile time. Zero ceremony, near-zero runtime cost.

```typescript
{{#include ../../examples/i18n/snippets.ts:overview-ui-strings}}
```

The same key-resolution and interpolation runs through the explicit `t()` API for non-UI strings:

```typescript
{{#include ../../examples/i18n/snippets.ts:overview-imports}}
```

## Design Principles

- **Zero ceremony**: String literals in UI components are automatically localizable keys
- **Compile-time diagnostics**: Missing and unused translation keys are reported as warnings during build (missing keys fall back to the default locale's text)
- **Embedded string table**: All translations baked into the binary as a flat 2D table. Near-zero runtime lookup cost
- **Native locale detection**: POSIX locale environment variables first (`LANGUAGE`, `LC_ALL`, `LC_MESSAGES`, `LANG`), then platform OS APIs — so CLI binaries respect the environment and GUI apps (which carry no locale env vars) use the OS setting

## Quick Start

### 1. Add i18n config to perry.toml

```toml
[i18n]
locales = ["en", "de"]
default_locale = "en"
```

### 2. Extract strings from your code

```bash
perry i18n extract src/main.ts
```

This scans your source files and creates `locales/en.json` and `locales/de.json`:

```json
// locales/en.json
{
  "Next": "Next",
  "Back": "Back"
}

// locales/de.json (empty values = needs translation)
{
  "Next": "",
  "Back": ""
}
```

### 3. Translate

Fill in `locales/de.json`:

```json
{
  "Next": "Weiter",
  "Back": "Zurck"
}
```

### 4. Build

```bash
perry compile src/main.ts -o myapp
```

Perry warns about missing or unused translations at compile time and bakes every configured locale's strings into the binary. At runtime, the app detects the user's locale and shows the right language.

## How It Works

1. **Detection**: String literals in UI component calls (`Button`, `Text`, `Label`, etc.) are automatically treated as i18n keys
2. **Transform**: The compiler replaces `Expr::String("Next")` with `Expr::I18nString { key: "Next", string_idx: 0 }` in the HIR
3. **Codegen**: For each `I18nString`, the compiler emits a locale branch that selects the correct translation at runtime (keys whose translation is identical in every locale skip the branch entirely)
4. **Locale detection**: At startup, the entry point calls `perry_i18n_init()`, which detects the system locale and sets the global locale index all `t()` calls, plural rules, and format wrappers read

## Locale Detection

Locale environment variables are checked first on **every** platform, in POSIX order: `LANGUAGE`, then `LC_ALL`, `LC_MESSAGES`, `LANG` (values `C` and `POSIX` are ignored). This makes `LANG=de_DE.UTF-8 ./myapp` work the way Unix CLI tools are expected to. When none are set — the normal case for GUI launches via Finder, SpringBoard, etc. — detection falls through to the platform API:

| Platform | Method |
|----------|--------|
| macOS / iOS / watchOS / tvOS / visionOS | `NSBundle.preferredLocalizations` (respects per-app language settings), falling back to `CFLocaleCopyCurrent()` |
| Android | `__system_property_get("persist.sys.locale")` (then `ro.product.locale`, `persist.sys.language`) |
| Windows | `GetUserDefaultLocaleName()` (Win32) |
| Linux | env vars only (above) |

The detected locale is fuzzy-matched against your configured locales: `de_DE.UTF-8` matches `de`, `en-US` matches `en`, etc. No match selects the configured `default_locale`. Set `PERRY_I18N_DEBUG=1` to write a detection log to `~/Documents/i18n-debug.log`.

## Platform Output

When compiling for mobile targets, Perry generates platform-native locale resources alongside the binary:

| Platform | Output |
|----------|--------|
| iOS/macOS | `{locale}.lproj/Localizable.strings` inside `.app` bundle |
| Android | `res/values-{locale}/strings.xml` |
| Desktop | Strings embedded in binary (no extra files) |

## Next Steps

- [Interpolation & Plurals](interpolation.md)
- [Formatting](formatting.md)
- [CLI Tools](cli.md)
