# Wear OS

Perry can compile TypeScript apps for Wear OS watches and the Wear OS emulator.

Wear OS **is Android on a watch**, so Perry reuses the exact same backend as the
[Android](android.md) target: your `perry/ui` tree lowers through `perry-ui-android`
(JNI → `TextView` / `LinearLayout` / `Button` / …), and the compiled
`aarch64-linux-android` `.so` is identical to a phone build. `perry run wearos`
then packages it with a **watch form-factor overlay** — the
`android.hardware.type.watch` feature, the standalone meta-data, and the
`androidx.wear` support library — and installs it over `adb` like any other APK.

> This page is about **full Wear OS apps**. For glanceable Wear OS **Tiles**
> (the swipe-left surfaces, built from `Widget({...})` declarations), see
> [Wear OS Tiles](../widgets/wearos.md).

## Requirements

Same toolchain as the [Android](android.md) target, plus a Wear OS system image:

- **JDK 17** and **Gradle** (`brew install --cask temurin@17` / `brew install gradle`)
- **Android SDK + NDK** (set `ANDROID_HOME` and `ANDROID_NDK_HOME`)
- The Rust Android target (Wear is arm64-only — NaN-boxing needs 64-bit pointers):
  ```bash
  rustup target add aarch64-linux-android
  ```
- A **Wear OS** system image + AVD:
  ```bash
  sdkmanager "platforms;android-35" "build-tools;35.0.0" \
             "ndk;27.1.12297006" \
             "system-images;android-34;android-wear;arm64-v8a"

  avdmanager create avd -n perry_wear \
    -k "system-images;android-34;android-wear;arm64-v8a" -d wearos_large_round
  emulator -avd perry_wear
  ```

On Apple-silicon Macs use the `arm64-v8a` Wear image (runs natively). On Intel
hosts use an `x86`/`x86_64` Wear image instead.

## Building

```bash
perry compile app.ts -o app --target wearos
```

This cross-compiles to `aarch64-linux-android` and emits `libperry_app.so` — the
same artifact as `--target android`. Packaging into a watch APK happens at run
time (below).

## Running with `perry run`

```bash
perry run wearos          # Auto-detect a connected watch / booted Wear emulator
```

`perry run wearos`:

1. Cross-compiles the `.so` (identical to the Android path).
2. Copies the Android Gradle template and applies the Wear overlay:
   - adds `<uses-feature android:name="android.hardware.type.watch" android:required="true" />`
   - adds `<meta-data android:name="com.google.android.wearable.standalone" android:value="true" />`
   - adds `implementation("androidx.wear:wear:1.3.0")`
   - raises `minSdk` to 30 (Wear OS 3, the Google Play floor for watch APKs)
3. Runs `./gradlew assembleDebug`, debug-signs, then `adb install` + launches and
   streams `logcat`.

`wear` and `wear-os` are accepted as aliases for `wearos`.

## UI Toolkit

Identical to [Android](android.md) — the same `perry/ui` widgets map to the same
Android `View` classes (`Text` → `TextView`, `VStack` → vertical `LinearLayout`,
`Button` → `Button`, `ScrollView` → `ScrollView`, and so on). No Wear-specific
widget API is required: an Android UI tree renders directly on a watch.

The `androidx.wear` dependency in the overlay brings in `BoxInsetLayout` and
swipe-to-dismiss so round screens and the back gesture behave like a native Wear
app.

## App Lifecycle

A Wear OS app uses the same `App({...})` entry point as every other Perry UI
target:

```typescript
{{#include ../../examples/platforms/ui/wearos_app.ts:wearos-app}}
```

Under the hood this is the Android lifecycle: `PerryActivity` loads
`libperry_app.so`, calls into your compiled `main()` over JNI, and the
`perry/ui` tree is realized as Android `View`s on the watch.

## State Management

Reactive state works exactly as on Android and the other platforms:

```typescript
{{#include ../../examples/platforms/ui/counter_app.ts:counter}}
```

## Platform Detection

Because the runtime target triple is `aarch64-linux-android`, a Wear OS app
reports the **Android** platform number — `__platform__ === 2`:

```typescript
{{#include ../../examples/platforms/platform_detect.ts:overview-detect}}
```

There is intentionally no separate Wear OS platform constant: at runtime a Wear
app *is* an Android app. Branch on screen size at runtime if you need
watch-specific layout.

## Configuration

Wear OS reuses the `[android]` section of `perry.toml` (bundle id, etc.):

```toml
[android]
bundle_id = "com.example.mywatch"
```

## Limitations

Wear OS inherits Android's widget set but the watch form factor imposes the usual
constraints:

- **Small round screens** — design for ~1.2–1.4" displays; prefer `ScrollView`
  and short labels. Use the `androidx.wear` `BoxInsetLayout` insets for round
  bezels.
- **Touch-only** — no hover or right-click.
- **Single window** — modal flows map to `Dialog` views, same as Android.
- **Battery / memory** — keep apps lightweight; Wear devices have far less RAM
  than phones.
- **Publishing** — Wear apps ship through Google Play as Android APK/AABs (the
  `perry publish` flow treats Wear like Android).

## Next Steps

- [Android](android.md) — the shared backend, UI mapping, and APK details
- [Wear OS Tiles](../widgets/wearos.md) — glanceable Tile surfaces
- [Platform Overview](overview.md) — all platforms
- [UI Overview](../ui/overview.md) — the `perry/ui` system
