# watchOS

Perry can compile TypeScript apps for Apple Watch devices and the watchOS Simulator.

Since watchOS does not support UIKit views, Perry uses a **data-driven SwiftUI renderer**: your TypeScript code builds a UI tree via the standard `perry/ui` API, and a fixed SwiftUI runtime (shipped with Perry) queries the tree and renders it reactively. No code generation or transpilation is involved — the binary is fully native.

## Requirements

- macOS host (cross-compilation from Linux/Windows is not supported)
- Xcode (full install) for watchOS SDK and Simulator
- Rust watchOS targets. The simulator target is tier 2 and can be added with
  `rustup`; the **device** targets are tier 3 and ship no prebuilt `std`, so
  their runtime libraries must be built from source with a nightly toolchain
  and `-Z build-std` (see [Building for Device](#building-for-device)):
  ```bash
  rustup target add aarch64-apple-watchos-sim       # simulator (tier 2)
  rustup component add rust-src --toolchain nightly  # for device build-std
  ```

## Watch architectures

watchOS spans two CPU architectures, and which one you target decides which
watches your app runs on:

| Architecture | Watches | watchOS | Perry target |
|---|---|---|---|
| **arm64** (64-bit) | Series 9/10/11, Ultra 2/3, SE 3 (S9 chip+) | 26+ | `--target watchos` (default) |
| **arm64_32** (ILP32, 32-bit pointers) | Series 4–8, SE 1/2 | 9–11 | `--target watchos` + `PERRY_WATCHOS_ARM64_32=1` |
| **arm64** (simulator) | — | — | `--target watchos-simulator` |

Apple moved S9-and-later watches to full arm64 in watchOS 26. Older watches stay
arm64_32 forever. Perry's NaN-boxed value representation is sound on both — a
32-bit pointer fits in the 48-bit NaN payload and clean tagged values round-trip
— but **heap-struct layouts are pointer-width-dependent**: any code that bakes in
a 64-bit field offset (the closure `type_tag`, the `ObjectHeader` field-region
base, …) reads the wrong bytes and segfaults on arm64_32 unless it derives the
offset from the target pointer width. See
`perry_runtime::closure::CLOSURE_TYPE_TAG_OFFSET` and `perry_codegen::target_layout`.
The simulator is always arm64 (Apple Silicon
host) and **cannot run an arm64_32 binary** — device-arch builds can only be
tested on real hardware (or shipped via TestFlight).

## Building for Simulator

```bash
perry compile app.ts -o app --target watchos-simulator
```

This produces an arm64 binary linked with `swiftc` against the watchOS Simulator
SDK, wrapped in a `.app` bundle.

## Building for Device

Device runtime libraries are tier-3 Rust targets with no prebuilt `std`, so build
`perry-runtime` (and `perry-ui-watchos`, if you use the SwiftUI tree renderer)
from source once, then point `PERRY_RUNTIME_DIR` at them:

```bash
# arm64 (Series 9+ / watchOS 26+) — the default device target
cargo +nightly build -Z build-std=std,panic_abort --release \
  -p perry-runtime -p perry-ui-watchos --target aarch64-apple-watchos

PERRY_RUNTIME_DIR=target/aarch64-apple-watchos/release \
  perry compile app.ts -o app --target watchos
```

```bash
# arm64_32 (Series 4-8 / SE) — opt in with PERRY_WATCHOS_ARM64_32
cargo +nightly build -Z build-std=std,panic_abort --release \
  -p perry-runtime -p perry-ui-watchos --target arm64_32-apple-watchos

PERRY_WATCHOS_ARM64_32=1 \
PERRY_RUNTIME_DIR=target/arm64_32-apple-watchos/release \
  perry compile app.ts -o app --target watchos
```

To support every watch from a single App Store upload, build **both** and `lipo`
them into a fat binary — see [Publishing to the App Store](watchos-app-store.md).

### Build environment variables

| Variable | Effect |
|---|---|
| `PERRY_WATCHOS_ARM64_32=1` | Switch the `watchos` device target from arm64 to arm64_32 (codegen object arch, runtime/native-lib/Swift/link triples, and the bundle's `MinimumOSVersion` floor all follow). |
| `PERRY_WATCHOS_MIN` | Override `MinimumOSVersion` for arm64_32 device builds (default `11.0`). The engine/SwiftUI you link may impose its own floor — e.g. `onChange(of:initial:)` needs watchOS 10. |
| `PERRY_ENTRY_SYMBOL` | Name the C entry symbol emitted by codegen instead of renaming `_main` afterwards. Needed on arm64_32 because `rust-objcopy --redefine-sym` segfaults on arm64_32 Mach-O (`MachOWriter::writeSections`); see below. |

> **arm64_32 entry symbol.** With `--features watchos-swift-app`/`watchos-game-loop`,
> Perry normally emits `_main` and renames it to `__perry_user_main` with
> `rust-objcopy`. That tool crashes on arm64_32 objects, so for arm64_32 set
> `PERRY_ENTRY_SYMBOL=_perry_user_main` — codegen then emits the final symbol
> directly (the leading underscore yields Mach-O `__perry_user_main`, which the
> Swift `@main` shell references via `@_silgen_name`) and Perry skips the objcopy
> pass. A fat `lipo` build needs the same symbol in both slices.

> **Note for runtime contributors.** arm64_32 has 32-bit `usize`. Pointer-range
> guards and size caps in `perry-runtime` must compare in `u64` (e.g.
> `(addr as u64) < 0x8000_0000_0000`) rather than writing bare `usize` literals
> ≥ 2³² — those are a hard "literal out of range" error on arm64_32 (and wasm32).
> Use `usize::try_from(...).unwrap_or(usize::MAX)` to saturate length caps like
> `1usize << 53`.
>
> **Hardcoded struct-field offsets are the other arm64_32 trap.** A heap header
> whose layout includes a pointer shifts on arm64_32 — e.g. `ClosureHeader`'s
> `type_tag` sits at +12 after an 8-byte `func_ptr` on 64-bit but at +8 after a
> 4-byte one on ILP32, and `ObjectHeader`'s field region starts at +24 on 64-bit
> but +20 on ILP32 (the trailing `keys_array` pointer is 4 bytes). NEVER hardcode
> such an offset: in `perry-runtime` use `std::mem::offset_of!` / `size_of`
> (these track the target); in `perry-codegen` (which runs on the host but emits
> for the target) derive it from the target triple via `crate::target_layout`.
> Hardcoded `12` (closure magic) and `24` (`ObjectHeader` size) were the original
> arm64_32 startup-crash root causes — a real getter failed its `CLOSURE_MAGIC`
> probe, was judged non-callable, and the resulting `TypeError` value-coercion
> dereferenced the closure as an object.

## Running with `perry run`

```bash
perry run watchos                # Auto-detect booted watch simulator
perry run watchos --simulator <UDID>  # Target a specific simulator
```

Perry auto-discovers booted Apple Watch simulators. To install and launch manually:

```bash
xcrun simctl install booted app_watchos/app.app
xcrun simctl launch booted com.perry.app
```

## UI Toolkit

Perry maps UI widgets to SwiftUI views via a data-driven bridge:

| Perry Widget | SwiftUI View | Notes |
|-------------|-------------|-------|
| Text | Text | Font size, weight, color, wrapping |
| Button | Button | Tap action via native closure callback |
| VStack | VStack | With spacing |
| HStack | HStack | With spacing |
| ZStack | ZStack | Layered views |
| Spacer | Spacer | |
| Divider | Divider | |
| Toggle | Toggle | Two-way state binding |
| Slider | Slider | Min/max/value, state binding |
| Image | Image(systemName:) | SF Symbols |
| ScrollView | ScrollView | |
| ProgressView | ProgressView | Linear |
| Picker | Picker | Selection list |
| Form | List | Maps to List on watchOS |
| NavigationStack | NavigationStack | Push navigation |

### Modifiers

All widgets support these styling modifiers:

- `foregroundColor` / `backgroundColor`
- `font` (size, weight, family)
- `frame` (width, height)
- `padding` (uniform or per-edge)
- `cornerRadius`
- `opacity`
- `hidden` / `disabled`

## App Lifecycle

watchOS apps use SwiftUI's `@main App` pattern. Perry's PerryWatchApp.swift runtime handles the app lifecycle automatically:

```typescript
{{#include ../../examples/platforms/ui/watchos_app.ts:watchos-app}}
```

Under the hood:
1. `perry_main_init()` runs your compiled TypeScript, which builds the UI tree in memory
2. The SwiftUI `@main` struct observes the tree version and renders it
3. User interactions (button taps, toggle changes) call back into native closures

## State Management

Reactive state works the same as other platforms:

```typescript
{{#include ../../examples/platforms/ui/counter_app.ts:counter}}
```

When `state.set()` is called, the tree version increments and SwiftUI re-renders the affected views automatically.

## How It Works

Unlike iOS (UIKit) and macOS (AppKit), where Perry calls native view APIs directly via FFI, watchOS uses a **data-driven architecture**:

```
TypeScript code
  |
  v
perry_ui_*() FFI calls  →  Node tree stored in memory (Rust)
                                      |
                                      v
                        PerryWatchApp.swift queries tree via FFI
                                      |
                                      v
                        SwiftUI renders views reactively
                                      |
                                      v
                        User interaction → FFI callback → native closure
```

The `PerryWatchApp.swift` file is a fixed runtime (~280 lines) that ships with Perry. It never changes per-app — it's the watchOS equivalent of `libperry_ui_ios.a`.

## App rendering modes

The data-driven SwiftUI renderer above is the default. Two feature flags switch
to app shells that own their own entry point — used by games and apps that draw
their own frames instead of building a `perry/ui` tree:

| Feature | Shell | Use case |
|---|---|---|
| *(default)* | Perry's `PerryWatchApp.swift` observes the UI tree | Standard `perry/ui` apps |
| `--features watchos-swift-app` | A native library ships its own `@main struct App: App` | Games / engines with a custom SwiftUI `Canvas` (e.g. Bloom Engine) |
| `--features watchos-game-loop` | `perry-runtime` provides C `main()` + `WKApplicationMain` | Metal/wgpu game loops |

In both non-default modes the TypeScript entry runs on a background thread the
shell spawns, and the shell references it as `__perry_user_main` (see
`PERRY_ENTRY_SYMBOL` above).

## Configuration

Configure watchOS settings in `perry.toml`:

```toml
[watchos]
bundle_id = "com.example.mywatch"
deployment_target = "10.0"

[watchos.info_plist]
NSLocationWhenInUseUsageDescription = "Used for location features"
```

Set up signing credentials with:

```bash
perry setup watchos
```

This shares App Store Connect credentials with iOS/macOS (same team, API key, issuer).

## Platform Detection

Use `__platform__ === 7` to detect watchOS at compile time:

```typescript
{{#include ../../examples/platforms/platform_detect.ts:watchos-detect}}
```

## watchOS Widgets (WidgetKit)

Perry also supports watchOS WidgetKit complications (separate from full apps):

```bash
perry compile widget.ts --target watchos-widget --app-bundle-id com.example.app
```

See [watchOS Complications](../widgets/watchos.md) for widget-specific documentation.

## Limitations

watchOS apps have inherent platform constraints compared to other Perry targets:

- **No Canvas**: CoreGraphics drawing is not available
- **No Camera**: watchOS does not support camera APIs
- **No TextField**: Text input is extremely limited on Apple Watch
- **No File Dialogs**: No document picker
- **No Menu Bar / Toolbar**: Not applicable on watch
- **No Multi-Window**: Single window only
- **No QR Code**: Screen too small for practical QR display
- **Memory**: watchOS devices have ~50-75MB available RAM — keep apps lightweight
- **Screen size**: Design for 40-49mm watch faces

## Differences from iOS

- **SwiftUI vs UIKit**: watchOS uses SwiftUI rendering; iOS uses UIKit directly
- **No splash screen**: watchOS apps don't use launch storyboards
- **Standalone**: watchOS apps are standalone (no iPhone companion required, `WKWatchOnly = true`)
- **Device family**: `UIDeviceFamily = [4]` (watch) vs `[1, 2]` (iPhone/iPad)

## Next Steps

- [Publishing watchOS apps to the App Store](watchos-app-store.md) — fat binaries, the iOS-stub wrapper, and signing for a watch-only app
- [watchOS Complications](../widgets/watchos.md) — WidgetKit complications
- [iOS](ios.md) — iOS platform reference
- [Platform Overview](overview.md) — All platforms
- [UI Overview](../ui/overview.md) — UI system
