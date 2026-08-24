# iOS

Perry can cross-compile TypeScript apps for iOS devices and the iOS Simulator.

## Requirements

- macOS host (cross-compilation from Linux/Windows is not supported)
- Xcode (full install, not just Command Line Tools) for iOS SDK and Simulator
- Rust iOS targets:
  ```bash
  rustup target add aarch64-apple-ios aarch64-apple-ios-sim
  ```

## Building for Simulator

```bash
perry app.ts -o app --target ios-simulator
```

This uses LLVM cross-compilation with the iOS Simulator SDK. The binary can be run in the Xcode Simulator.

## Building for Device

```bash
perry app.ts -o app --target ios
```

This produces an ARM64 binary for physical iOS devices. You'll need to code sign and package it in an `.app` bundle for deployment.

## Running with `perry run`

The easiest way to build and run on iOS is `perry run`:

```bash
perry run ios              # Auto-detect device/simulator
perry run ios --console    # Stream live stdout/stderr
perry run ios --remote     # Use Perry Hub build server
```

Perry auto-discovers available simulators (via `simctl`) and physical devices (via `devicectl`). When multiple targets are found, an interactive prompt lets you choose.

For physical devices, Perry handles code signing automatically — it reads your signing identity and team ID from `~/.perry/config.toml` (set up via `perry setup ios`), embeds the provisioning profile, and signs the `.app` before installing.

If you don't have the iOS cross-compilation toolchain installed locally, `perry run ios` automatically falls back to Perry Hub's remote build server.

## UI Toolkit

Perry maps UI widgets to UIKit controls:

| Perry Widget | UIKit Class |
|-------------|------------|
| Text | UILabel |
| Button | UIButton (TouchUpInside) |
| TextField | UITextField |
| SecureField | UITextField (secureTextEntry) |
| Toggle | UISwitch |
| Slider | UISlider (Float32, cast at boundary) |
| Picker | UIPickerView |
| Image | UIImageView |
| VStack/HStack | UIStackView |
| ScrollView | UIScrollView |

## App Lifecycle

iOS apps use `UIApplicationMain` with a deferred creation pattern:

```typescript
{{#include ../../examples/platforms/ui/ios_app.ts:ios-app}}
```

The `App()` call triggers `UIApplicationMain`, and your render function is called via `PerryAppDelegate` once the app is ready. Perry-generated apps use `UIWindowScene`, `PerrySceneDelegate`, and an `UIApplicationSceneManifest`, which also satisfies the scene-based lifecycle required for apps built with the iOS 27 SDK.

## Adaptive layouts

Use `perry/ios` to inspect the active scene rather than branching on a device model or physical screen size:

```typescript,no-test
import {
  getLayoutEnvironment,
  onLayoutChange,
  offLayoutChange,
} from "perry/ios";

const initial = getLayoutEnvironment();
console.log(initial.width, initial.horizontalSizeClass, initial.windowMode);

const subscription = onLayoutChange((layout) => {
  if (layout.horizontalSizeClass === "compact") {
    // Present a compact navigation treatment.
  }
  if (layout.isFourByThree || layout.windowMode === "sideBySide") {
    // Reflow content for 4:3 or iPad side-by-side multitasking.
  }

  // These insets describe display cutouts, rounded corners, and any future
  // interrupted-display geometry exposed to the scene by UIKit.
  console.log(layout.safeAreaTop, layout.safeAreaRight);
});

// When the observer is no longer needed:
offLayoutChange(subscription);
```

Snapshots contain the window dimensions and aspect ratio, display scale, horizontal and vertical size classes, orientation, window mode, multitasking and 4:3 flags, and all four safe-area insets. On iOS 27 they also contain the effective scene's system-space frame, interactive-resize state, and orientation-lock state. The callback fires once when a scene is available and then after meaningful bounds, trait, safe-area, or effective-geometry changes.

UIKit does not expose a separate public hardware-model or hinge-state property. Safe areas, effective scene geometry, and trait collections are the supported adaptive signals, and they continue to work when one device moves among full-screen, side-by-side, and freeform window modes. Perry's `SplitView` and `FrameSplit` also cap their preferred sidebar at 45% of the current scene width so the detail pane remains usable in narrow layouts.

## Foundation Models

The simple, unstructured Foundation Models flow is available through `perry/ios`:

```typescript,no-test
import {
  foundationModelAvailability,
  createLanguageModelSession,
  respond,
  destroyLanguageModelSession,
} from "perry/ios";

if (foundationModelAvailability() === "available") {
  const session = createLanguageModelSession(
    "Answer in one short, factual sentence.",
  );
  try {
    const answer = await respond(session, "Why is the sky blue?");
    console.log(answer);
  } finally {
    destroyLanguageModelSession(session);
  }
}
```

The bridge uses Apple's default `LanguageModelSession`, preserves conversational context while a session handle is reused, and rejects the returned promise when generation fails. Check availability first: unsupported OS versions and unavailable Apple Intelligence configurations are reported without loading the framework. This surface intentionally returns plain strings; structured `@Generable` responses are outside the current API.

Building a source file that imports `perry/ios` requires an Xcode SDK containing `FoundationModels.framework` (Xcode 26 or later). The framework is weak-linked, so the normal iOS 17 deployment target remains valid.

## Now Playing on iOS 27

`perry/media.setNowPlaying(...)` is the public Perry API for Lock Screen, Control Center, Dynamic Island, CarPlay, artwork, playback progress, and play/pause/stop/seek commands. When an Xcode 27 SDK containing `NowPlaying.framework` is installed, Perry automatically compiles an observable `MediaSession` bridge and publishes each player through the new framework. Builds made with older SDKs, and devices before iOS 27, retain the existing `MPNowPlayingInfoCenter` / `MPRemoteCommandCenter` compatibility path. Perry never activates both paths for one local session.

The iOS 27 SDK is beta software until Apple's GM release. This support does not change Perry's SDK build markers or version; distribution metadata should only be updated once the GM toolchain can submit to App Store Connect.

## iOS Widgets (WidgetKit)

Perry can compile TypeScript widget declarations to native SwiftUI WidgetKit extensions:

```bash
perry widget.ts --target ios-widget
```

See [Widgets (WidgetKit)](../widgets/overview.md) for details.

## Splash Screen

Perry auto-generates a native `LaunchScreen.storyboard` from the `perry.splash` config in `package.json`. The splash screen appears instantly during cold start.

```json
{
  "perry": {
    "splash": {
      "image": "logo/icon-256.png",
      "background": "#FFF5EE"
    }
  }
}
```

The image is centered at 128x128pt with `scaleAspectFit`. You can provide a custom storyboard for full control:

```json
{
  "perry": {
    "splash": {
      "ios": { "storyboard": "splash/LaunchScreen.storyboard" }
    }
  }
}
```

See [Project Configuration](../getting-started/project-config.md#splash) for the full config reference.

## Resource Bundling

Perry automatically bundles `logo/` and `assets/` directories from your project root into the `.app` bundle. These resources are available at runtime via standard file APIs relative to the app bundle path.

## Keyboard Avoidance

Perry apps automatically handle keyboard avoidance on iOS. When the keyboard appears, the root view adjusts its bottom constraint with an animated layout transition, and focused TextFields are auto-scrolled into view above the keyboard.

## Differences from macOS

- **No menu bar**: iOS doesn't support menu bars. Use toolbar or navigation patterns.
- **Touch events**: `onHover` is not available. Use `onClick` (mapped to touch).
- **Slider precision**: iOS UISlider uses Float32 internally (automatically converted).
- **File dialogs**: Limited to UIDocumentPicker.
- **Keyboard shortcuts**: Not applicable on iOS.

## Next Steps

- [Widgets (WidgetKit)](../widgets/overview.md) — iOS home screen widgets
- [Platform Overview](overview.md) — All platforms
- [UI Overview](../ui/overview.md) — UI system
