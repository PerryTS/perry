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
| `windows` | Windows | Win32/GDI executable (default Windows backend) |
| `windows-winui` | Windows (Fluent) | Opt-in WinUI 3 / Fluent backend (#4680). **Scaffold:** currently renders via Win32 while the XAML widget mapping lands incrementally; selects the `perry-ui-windows-winui` static library. Build that lib first: `cargo build --release -p perry-ui-windows-winui`. |
| `linux` | Linux | GTK4 executable |

## Output Types

Use `--output-type` to change what's produced:

| Type | Description |
|------|-------------|
| `executable` | Standalone binary (default) |
| `dylib` | Shared library (`.dylib`/`.so`) for [plugins](../plugins/overview.md) |

## Embedding Assets

Bake static files (an SPA `dist/`, images, JSON, fonts, …) into the standalone
executable so it runs with no external files on disk (#5731).

| Flag | Description |
|------|-------------|
| `--embed <pattern>` | Embed a file, directory, or `*`/`**` glob (relative to the project root). Repeatable. Merged with `perry.embed` (package.json) and `[compile] embed` (perry.toml). |

```bash
vite build
perry compile server.ts --embed "./dist/**" -o myapp
./myapp   # serves dist/ from memory — no dist/ folder needed
```

Embedded files are reachable at runtime three ways:

```ts
import { embeddedFiles, readEmbedded, isStandaloneExecutable } from "perry";
import { readFileSync } from "fs";

for (const f of embeddedFiles()) {
  // f.name (e.g. "dist/index.html"), f.size, f.type (MIME)
  app.get("/" + f.name, (_, reply) => reply.type(f.type).send(readEmbedded(f.name)));
}

// or via node:fs at the `$perryfs/<path>` virtual path:
const html = readFileSync("$perryfs/dist/index.html", "utf8");
```

`embeddedFiles()` is a function (not a bare value like Bun's `embeddedFiles`) so
that array methods dispatch on its result. `readEmbedded(path)` and `node:fs`
accept either the `$perryfs/<path>` virtual path or the embed-relative key.

> **Note**
> `node:fs` consults the embedded registry *before* disk, and a bare
> embed-relative key matches too — so `readFileSync("dist/index.html")` returns
> the **embedded** bytes even if a `dist/index.html` exists on disk next to the
> binary. Read a real on-disk file by absolute path, and use the explicit
> `$perryfs/<path>` form when you specifically mean the embedded copy.
>
> Embedding currently requires a Unix-like host toolchain (macOS/Linux); on a
> Windows host `--embed` errors out. Cross-target / Windows embedding is a
> tracked follow-up.

## Debug Flags

| Flag | Description |
|------|-------------|
| `--print-hir` | Print HIR (intermediate representation) to stdout |
| `--trace <STAGES>` | Dump IR at one or more pipeline stages. Comma-separated: `hir` (post-transform HIR), `llvm` (per-module `.ll` into `.perry-trace/llvm/`), or `all` |
| `--focus <NAME>` | Restrict `--trace hir` to functions/methods/classes whose name contains `NAME`, suppressing import/export/init noise. Implies `--trace hir` if no stage is given |
| `--no-link` | Produce `.o` object file only, skip linking |
| `--no-codegen` | Skip the `package.json` `perry.codegen` build-time steps (also `PERRY_SKIP_CODEGEN=1`). See [Project Configuration](../getting-started/project-config.md) |
| `--keep-intermediates` | Keep `.o` and `.asm` intermediate files |

The `--trace`/`--focus` pair localizes "compiled to the wrong thing" bugs:
`perry compile foo.ts --trace hir,llvm --focus parseRow` dumps just the
`parseRow` function's lowered HIR and the module's LLVM IR, so you can see
which stage corrupted it without scrolling a full-module dump. `--trace llvm`
forces a full recompile (the object cache otherwise skips codegen for
unchanged modules, leaving the trace dir empty).

## Output Optimization

| Flag | Description |
|------|-------------|
| `--minify` | Minify and obfuscate output (auto-enabled for `--target web`) |
| `--march <CPU>` | CPU baseline for the generated machine code: an LLVM CPU name (`x86-64-v2`, `x86-64-v3`, `znver2`, `apple-m1`, …), `native` (tune to the build machine — the default for host builds), or `generic` (the target architecture's portable baseline — the default for cross builds). Pin this when the binary runs on other machines: a host-native build on an AVX-512 box otherwise SIGILLs on older x86-64 CPUs. Also settable via `PERRY_TARGET_CPU` or perry.toml `[build] march`; `[build] native_tuning = false` is shorthand for `generic`. Applies to app code and the auto-optimized runtime/stdlib rebuild. |

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
| `--enable-wasm-runtime` | Force-link the wasmi WebAssembly host runtime (auto-detected when `WebAssembly.*` is referenced; needed only when loading via dlopen / FFI without a static reference) |
| `--type-check` | Enable type checking via tsgo IPC |
| `--strict-eval` | Fail the build if any runtime-unknown `eval(...)` / `new Function(<dynamic body>)` site is reachable. By default such a site is compiled to a deferred runtime error (throws only if reached) and a compile-time notice is printed. Also settable via `perry.eval = "error"` / `perry.strict = true` (package.json or perry.toml). `PERRY_ALLOW_EVAL=1` forces it off. See [Limitations](../language/limitations.md#no-eval-or-dynamic-code). |
| `--strict-dynamic-import` | Fail the build if a dynamic `import(...)` has a runtime-computed (non-resolvable) specifier. By default such a site is compiled to a rejected `Promise` that throws a descriptive `Error` only if reached, and is listed in the same end-of-build notice as deferred eval sites. Also settable via `perry.dynamicImport = "error"` / `perry.strict = true` (package.json or perry.toml). `PERRY_ALLOW_EVAL=1` forces it off. Resolvable forms (string literals, ternaries of resolvable arms, template literals over const locals, finite union-typed params, glob) are unaffected. See [Limitations](../language/limitations.md#no-eval-or-dynamic-code). |

## Environment Variables

| Variable | Description |
|----------|-------------|
| `PERRY_LICENSE_KEY` | Perry Hub license key for `perry publish` |
| `PERRY_APPLE_CERTIFICATE_PASSWORD` | Password for .p12 certificate |
| `PERRY_TARGET_CPU` | CPU baseline for generated machine code (same values as `--march`; the flag and perry.toml `[build] march` win over the env var) |
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

[compile]
# Embed static assets into the standalone executable (same as repeated --embed).
embed = ["./dist/**"]

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
