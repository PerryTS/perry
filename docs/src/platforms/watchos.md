# watchOS

Perry can compile TypeScript apps for Apple Watch devices and the watchOS Simulator.

Since watchOS does not support UIKit views, Perry uses a **data-driven SwiftUI renderer**: your TypeScript code builds a UI tree via the standard `perry/ui` API, and a fixed SwiftUI runtime (shipped with Perry) queries the tree and renders it reactively. No code generation or transpilation is involved — the binary is fully native.

## Requirements

- macOS host (cross-compilation from Linux/Windows is not supported)
- Xcode (full install) for watchOS SDK and Simulator
- Rust watchOS targets:
  ```bash
  rustup target add arm64_32-apple-watchos aarch64-apple-watchos-sim
  ```

## Building for Simulator

```bash
perry compile app.ts -o app --target watchos-simulator
```

This produces an ARM64 binary linked with `swiftc` against the watchOS Simulator SDK, wrapped in a `.app` bundle.

## Building for Device

```bash
perry compile app.ts -o app --target watchos
```

This produces an arm64_32 (ILP32) binary for physical Apple Watch hardware. Apple Watch uses 32-bit pointers on 64-bit ARM.

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

```typescript,no-test
import { App, Text, VStack, Button } from "perry/ui";

App({
  title: "My Watch App",
  width: 200,
  height: 200,
  body: VStack(8, [
    Text("Hello, Apple Watch!"),
    Button("Tap me", () => {
      console.log("Button tapped!");
    }),
  ]),
});
```

Under the hood:
1. `perry_main_init()` runs your compiled TypeScript, which builds the UI tree in memory
2. The SwiftUI `@main` struct observes the tree version and renders it
3. User interactions (button taps, toggle changes) call back into native closures

## State Management

Reactive state works the same as other platforms:

```typescript,no-test
import { App, Text, VStack, Button, State } from "perry/ui";

const count = State(0);

App({
  title: "Counter",
  width: 200,
  height: 200,
  body: VStack(8, [
    Text(`Count: ${count.value}`),
    Button("+1", () => {
      count.set(count.value + 1);
    }),
  ]),
});
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

```typescript,no-test
declare const __platform__: number;

if (__platform__ === 7) {
  console.log("Running on watchOS");
}
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

- [watchOS Complications](../widgets/watchos.md) — WidgetKit complications
- [iOS](ios.md) — iOS platform reference
- [Platform Overview](overview.md) — All platforms
- [UI Overview](../ui/overview.md) — UI system
