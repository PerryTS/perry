# Platform Overview

Perry compiles TypeScript to native executables for 9 platform families from the same source code.

## Supported Platforms

| Platform | Target Flag | UI Toolkit | Status |
|----------|-------------|------------|--------|
| macOS | *(default)* | AppKit | Full support (127/127 FFI functions) |
| iOS | `--target ios` / `--target ios-simulator` | UIKit | Full support (127/127) |
| visionOS | `--target visionos` / `--target visionos-simulator` | UIKit (2D windows) | Core support (2D only) |
| tvOS | `--target tvos` / `--target tvos-simulator` | UIKit | Full support (focus engine + game controllers) |
| watchOS | `--target watchos` / `--target watchos-simulator` | SwiftUI (data-driven) | Core support (15 widgets) |
| Android | `--target android` | JNI/Android SDK | Full support (112/112) |
| Windows | `--target windows` | Win32 | Full support (112/112) |
| Linux | `--target linux` | GTK4 | Full support (112/112) |
| Web / WebAssembly | `--target web` *(alias `--target wasm`)* | DOM/CSS via WASM bridge | Full support (168 widgets) |

## Cross-Compilation

```bash
# Default: compile for current platform
perry app.ts -o app

# Compile for a specific target
perry app.ts -o app --target ios-simulator
perry app.ts -o app --target visionos-simulator
perry app.ts -o app --target tvos-simulator
perry app.ts -o app --target watchos-simulator
perry app.ts -o app --target web   # alias: --target wasm
perry app.ts -o app --target windows
perry app.ts -o app --target linux
perry app.ts -o app --target android
```

## Platform Detection

Use the `__platform__` compile-time constant to branch by platform:

```typescript,no-test
declare const __platform__: number;

// Platform constants:
// 0 = macOS
// 1 = iOS
// 2 = Android
// 3 = Windows
// 4 = Linux
// 5 = Web (browser, --target web / --target wasm)
// 6 = tvOS
// 7 = watchOS
// 8 = visionOS

if (__platform__ === 0) {
  console.log("Running on macOS");
} else if (__platform__ === 1) {
  console.log("Running on iOS");
} else if (__platform__ === 3) {
  console.log("Running on Windows");
}
```

`__platform__` is resolved at compile time. The compiler constant-folds comparisons and eliminates dead branches, so platform-specific code has zero runtime cost.

## Platform Feature Matrix

| Feature | macOS | iOS | visionOS | tvOS | watchOS | Android | Windows | Linux | Web (WASM) |
|---------|-------|-----|----------|------|---------|---------|---------|-------|------------|
| CLI programs | Yes | — | — | — | — | — | Yes | Yes | — |
| Native UI (DOM on web) | Yes | Yes | Yes | Yes | Yes | Yes | Yes | Yes | Yes |
| Game engines | Yes | Yes | — | Yes | — | Yes | Yes | Yes | Via FFI |
| File system | Yes | Sandboxed | Sandboxed | Sandboxed | — | Sandboxed | Yes | Yes | File System Access API |
| Networking | Yes | Yes | Yes | Yes | Yes | Yes | Yes | Yes | `fetch` / `WebSocket` |
| System APIs | Yes | Partial | Partial | Partial | Minimal | Partial | Yes | Yes | Partial |
| Widgets (WidgetKit) | — | Yes | — | — | Yes | — | — | — | — |
| Threading | Native | Native | Native | Native | Native | Native | Native | Native | Web Workers |

## Next Steps

- [macOS](macos.md)
- [iOS](ios.md)
- [visionOS](visionos.md)
- [tvOS](tvos.md)
- [watchOS](watchos.md)
- [Android](android.md)
- [Windows](windows.md)
- [Linux (GTK4)](linux.md)
- [Web](web.md)
- [WebAssembly](wasm.md)
