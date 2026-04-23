# Compiler Flags

Complete reference for all Perry CLI flags.

## Global Flags

Available on all commands:

| Flag | Description |
|------|-------------|
| `--format text\|json` | Output format (default: `text`) |
| `-v, --verbose` | Increase verbosity (repeatable: `-v`, `-vv`, `-vvv`) |
| `-q, --quiet` | Suppress non-error output |
| `--no-color` | Disable ANSI color codes |

## Compilation Targets

Use `--target` to cross-compile:

| Target | Platform | Notes |
|--------|----------|-------|
| *(none)* | Current platform | Default behavior |
| `ios-simulator` | iOS Simulator | ARM64 simulator binary |
| `ios` | iOS Device | ARM64 device binary |
| `visionos-simulator` | visionOS Simulator | Apple Vision Pro simulator build |
| `visionos` | visionOS Device | Apple Vision Pro device build |
| `android` | Android | ARM64/ARMv7 |
| `ios-widget` | iOS Widget | WidgetKit extension (requires `--app-bundle-id`) |
| `ios-widget-simulator` | iOS Widget (Sim) | Widget for simulator |
| `watchos-widget` | watchOS Complication | WidgetKit extension for Apple Watch |
| `watchos-widget-simulator` | watchOS Widget (Sim) | Widget for watchOS simulator |
| `android-widget` | Android Widget | Android App Widget (AppWidgetProvider) |
| `wearos-tile` | Wear OS Tile | Wear OS Tile (TileService) |
| `wasm` | WebAssembly | Self-contained HTML with WASM or raw `.wasm` binary |
| `web` | Web | Outputs HTML file with JS |
| `windows` | Windows | Win32 executable |
| `linux` | Linux | GTK4 executable |

## Output Types

Use `--output-type` to change what's produced:

| Type | Description |
|------|-------------|
| `executable` | Standalone binary (default) |
| `dylib` | Shared library (`.dylib`/`.so`) for [plugins](../plugins/overview.md) |

## Debug Flags

| Flag | Description |
|------|-------------|
| `--print-hir` | Print HIR (intermediate representation) to stdout |
| `--no-link` | Produce `.o` object file only, skip linking |
| `--keep-intermediates` | Keep `.o` and `.asm` intermediate files |

## Output Optimization

| Flag | Description |
|------|-------------|
| `--minify` | Minify and obfuscate output (auto-enabled for `--target web`) |

Minification strips comments, collapses whitespace, and mangles local variable/parameter/non-exported function names for smaller output.

## Testing Flags

| Flag | Description |
|------|-------------|
| `--enable-geisterhand` | Embed the [Geisterhand](../testing/geisterhand.md) HTTP server for programmatic UI testing (default port 7676) |
| `--geisterhand-port <PORT>` | Set a custom port for the Geisterhand server (implies `--enable-geisterhand`) |

## Runtime Flags

| Flag | Description |
|------|-------------|
| `--enable-js-runtime` | Enable V8 JavaScript runtime for unsupported npm packages |
| `--type-check` | Enable type checking via tsgo IPC |

## Environment Variables

| Variable | Description |
|----------|-------------|
| `PERRY_LICENSE_KEY` | Perry Hub license key for `perry publish` |
| `PERRY_APPLE_CERTIFICATE_PASSWORD` | Password for .p12 certificate |
| `PERRY_NO_UPDATE_CHECK=1` | Disable automatic update checks |
| `PERRY_UPDATE_SERVER` | Custom update server URL |
| `CI=true` | Auto-skip update checks (set by most CI systems) |
| `RUST_LOG` | Debug logging level (`debug`, `info`, `trace`) |

## Configuration Files

### perry.toml (project)

```toml
[project]
name = "my-app"
entry = "src/main.ts"
version = "1.0.0"

[build]
out_dir = "build"

[app]
name = "My App"
description = "A Perry application"

[macos]
bundle_id = "com.example.myapp"
category = "public.app-category.developer-tools"
minimum_os = "13.0"
distribute = "notarize"  # "appstore", "notarize", or "both"

[ios]
bundle_id = "com.example.myapp"
deployment_target = "16.0"
device_family = ["iphone", "ipad"]

[android]
package_name = "com.example.myapp"
min_sdk = 26
target_sdk = 34

[linux]
format = "appimage"  # "appimage", "deb", "rpm"
category = "Development"
```

### ~/.perry/config.toml (global)

```toml
[apple]
team_id = "XXXXXXXXXX"
signing_identity = "Developer ID Application: Your Name"

[android]
keystore_path = "/path/to/keystore.jks"
key_alias = "my-key"
```

## Examples

```bash
# Simple CLI program
perry main.ts -o app

# iOS app for simulator
perry app.ts -o app --target ios-simulator

# visionOS app for simulator
perry app.ts -o app --target visionos-simulator

# Web app (WASM with DOM bridge — alias: --target wasm)
perry app.ts -o app --target web

# Plugin shared library
perry plugin.ts --output-type dylib -o plugin.dylib

# iOS widget with bundle ID
perry widget.ts --target ios-widget --app-bundle-id com.example.app

# Debug compilation
perry app.ts --print-hir 2>&1 | less

# Verbose compilation
perry compile app.ts -o app -vvv

# Type-checked compilation
perry app.ts -o app --type-check

# Raw WASM binary (no HTML wrapper)
perry app.ts -o app.wasm --target wasm

# Minified web output (compresses embedded JS bridge)
perry app.ts -o app --target web --minify
```

## Next Steps

- [Commands](commands.md) — All CLI commands
- [Platform Overview](../platforms/overview.md) — Platform targets
