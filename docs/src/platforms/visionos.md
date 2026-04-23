# visionOS

Perry can compile TypeScript apps for Apple Vision Pro devices and the visionOS Simulator.

This first pass targets **2D windowed apps only**. Perry uses the same UIKit-style `perry/ui` model as iOS, packaged for visionOS app bundles and scene lifecycle.

## Prerequisites

- macOS with Xcode installed
- Rust visionOS targets:

```bash
rustup target add aarch64-apple-visionos aarch64-apple-visionos-sim
```

## Compile

```bash
perry compile app.ts -o app --target visionos-simulator
perry compile app.ts -o app --target visionos
```

This produces a `.app` bundle with visionOS-specific `Info.plist` metadata and a `UIWindowScene` configuration.

## Run

```bash
perry run visionos
perry run visionos --simulator <UDID>
perry run visionos --device <UDID>
```

Perry auto-detects booted Apple Vision Pro simulators via `simctl`. Physical device installs use `devicectl`, like other modern Apple platforms.

## Configuration

Configure visionOS-specific settings in `perry.toml`:

```toml
[visionos]
bundle_id = "com.example.myvisionapp"
deployment_target = "1.0"
entry = "src/main_visionos.ts"
encryption_exempt = true
```

Custom `Info.plist` keys can be merged through `[visionos.info_plist]`.

## Platform Detection

Use `__platform__ === 8` to detect visionOS at compile time:

```typescript,no-test
declare const __platform__: number;

if (__platform__ === 8) {
  console.log("Running on visionOS");
}
```

## Current Scope

- Supported: 2D windowed apps, simulator/device app bundles, `perry run`, `perry setup`, `perry publish`
- Not supported yet: immersive spaces, volumes, RealityKit scene generation, Geisterhand

## Related

- [iOS](ios.md) — shared UIKit foundation
- [Platform Overview](overview.md)
