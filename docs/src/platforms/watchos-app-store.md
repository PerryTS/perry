# Publishing watchOS Apps to the App Store

Shipping a **watch-only** app (no iPhone app) through App Store Connect has two
non-obvious requirements that aren't enforced until you upload. This page covers
both, plus the architecture/deployment-target rules that decide which watches
your build reaches.

## Architecture rules

App Store validation enforces two rules for the watch app binary:

- **arm64 is required for every watchOS app**, always.
- **arm64_32 is *additionally* required when `MinimumOSVersion < 27.0`.**

So there are exactly two valid shapes:

| Build | `MinimumOSVersion` | Reaches |
|---|---|---|
| **Fat: arm64 + arm64_32** | < 27 (e.g. 10.0) | Every watch from Series 4 to the latest |
| arm64-only | ≥ 27.0 | Series 9+ only (watchOS 27+) |

An arm64_32-only upload is **rejected** ("missing arm64 architecture"). For the
widest reach, ship the fat binary with a low deployment target.

> **arm64-only builds can stall in processing.** A build whose
> `MinimumOSVersion` is a watchOS version that is not yet generally available
> (e.g. 27.0 during its beta period) may sit in "Processing" indefinitely —
> Apple's pipeline appears unable to finish it until that OS ships. A fat build
> targeting a shipped watchOS (e.g. 10.0) processes normally in minutes. Prefer
> the fat/low-minOS shape unless you specifically need arm64-only.

## Building the fat binary

Build each slice (see [Building for Device](watchos.md#building-for-device)),
re-stamp the arm64 slice's load command down to the shared deployment target
(it only ever runs on watchOS 26+ hardware, so the stamp is cosmetic), then
`lipo` them together:

```bash
# 1. arm64_32 slice at the shared deployment target (e.g. 10.0)
PERRY_WATCHOS_ARM64_32=1 PERRY_WATCHOS_MIN=10.0 \
PERRY_ENTRY_SYMBOL=_perry_user_main \
PERRY_RUNTIME_DIR=.../arm64_32-apple-watchos/release \
  perry compile app.ts -o AppA32 --target watchos --features watchos-swift-app

# 2. arm64 slice (default device target)
PERRY_RUNTIME_DIR=.../aarch64-apple-watchos/release \
  perry compile app.ts -o AppA64 --target watchos --features watchos-swift-app

# 3. align minos + fuse
xcrun vtool -set-build-version watchos 10.0 26.5 -replace \
  -output AppA64.min10 AppA64.app/AppA64
lipo -create -output App.fat AppA32.app/AppA32 AppA64.min10
lipo -info App.fat   # => arm64_32 arm64
```

Place `App.fat` as the watch app's executable and set the bundle's
`MinimumOSVersion` to the same value (10.0 here).

## The iOS stub wrapper

App Store Connect has no standalone "watchOS" platform — watch software ships
**inside an iOS app record**. Uploading a bare watch `.app` fails in Transporter
with `Unknown platform alias: watchOS`. The watch app must be nested in a minimal
iOS "stub" container:

```
Payload/
  <Container>.app/              # iOS stub (com.example.app)
    <stub binary>               # trivial UIKit app, never launched
    Info.plist
    Watch/
      <WatchApp>.app/           # the real watch app (com.example.app.watchkitapp)
        <fat binary>
        Info.plist
        embedded.mobileprovision
      embedded.mobileprovision
```

Xcode generates this stub automatically for "Watch-Only App" projects; with
Perry you assemble it by hand. The stub is a do-nothing Swift `UIApplicationDelegate`
compiled for `arm64-apple-ios`.

### Required Info.plist keys

**Watch app** (`Watch/<WatchApp>.app/Info.plist`):

| Key | Value |
|---|---|
| `WKApplication` | `true` |
| `WKWatchOnly` | `true` |
| `CFBundleIdentifier` | `com.example.app.watchkitapp` |
| `MinimumOSVersion` | matches the fat binary (e.g. `10.0`) |
| `UIDeviceFamily` | `[4]` |

Do **not** set `WKCompanionAppBundleIdentifier` when `WKWatchOnly` is true.

**Stub container** (`<Container>.app/Info.plist`):

| Key | Value |
|---|---|
| `ITSWatchOnlyContainer` | `true` |
| `LSApplicationLaunchProhibited` | `true` |
| `CFBundleIdentifier` | `com.example.app` |
| `UISupportedInterfaceOrientations` | all four orientations (iPad multitasking rule with `UIDeviceFamily [1,2]`) |

## Signing

The watch app and the stub each need their own distribution provisioning
profile and matching bundle ID, both signed with an **Apple Distribution**
identity. `WKWatchOnly` is rejected on an app record that already distributes an
iOS build, so a watch-only app needs its **own new app record** in App Store
Connect.

```bash
codesign --force --sign "Apple Distribution: <Team>" \
  --entitlements watch.entitlements "Container.app/Watch/WatchApp.app"
codesign --force --sign "Apple Distribution: <Team>" \
  --entitlements stub.entitlements  "Container.app"
codesign --verify --deep --strict "Container.app"
```

## Uploading

`altool` cannot upload watch apps ("cannot determine platform"). Use Transporter:

```bash
mkdir Payload && cp -R "Container.app" Payload/ && zip -qr App.ipa Payload
iTMSTransporter -m upload -assetFile App.ipa \
  -apiKey <KEY_ID> -apiIssuer <ISSUER_ID>
```

Transporter runs the architecture/plist validation above before accepting the
upload, so its errors are the fastest way to confirm the bundle is well-formed.

## Development install (no App Store)

To run a device build on a watch you own without TestFlight, sign it with a
**development** profile that lists the watch's UDID and `get-task-allow=true`,
then install via `devicectl`:

```bash
xcrun devicectl device install app --device <watch-udid> WatchApp.app
```

This requires **Developer Mode** enabled on the watch *and* its developer disk
image mounted — open **Xcode → Window → Devices and Simulators** and select the
watch once to mount it (`devicectl` reports `ddiServicesAvailable: false` until
then). Note this needs a watch matching your build's architecture: an arm64_32
build for a pre-S9 watch, an arm64 build for S9+.
