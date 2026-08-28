# Cross-Platform Reference

Perry widgets compile from a single TypeScript source to four platforms. The same `Widget({...})` declaration produces native code for each target.

> **Status:** this page has no TypeScript fences (only target-flag tables and
> shell build commands), so the doc-tests harness has nothing to run here. The
> `--target` flags listed below are wired in the compiler. Example files can
> provide the required bundle id with the harness's `// widget-bundle-id:`
> banner; end-to-end target coverage also requires the matching Xcode or
> Android SDK on the CI host.

## Target Flags

| Platform | Target Flag | Output |
|----------|------------|--------|
| iOS | `--target ios-widget` | SwiftUI `.swift` + Info.plist |
| iOS Simulator | `--target ios-widget-simulator` | Same, simulator SDK |
| Android | `--target android-widget` | Kotlin/Glance `.kt` + widget_info XML |
| watchOS | `--target watchos-widget` | SwiftUI `.swift` (accessory families) |
| watchOS Simulator | `--target watchos-widget-simulator` | Same, simulator SDK |
| Wear OS | `--target wearos-tile` | Kotlin Tiles `.kt` + manifest |

## Feature Matrix

| Feature | iOS | Android | watchOS | Wear OS |
|---------|-----|---------|---------|---------|
| Text | Yes | Yes | Yes | Yes |
| VStack/HStack/ZStack | Yes | Column/Row/Box | Yes | Column/Row/Box |
| Image (SF Symbols) | Yes | R.drawable | Yes | R.drawable |
| Spacer | Yes | Yes | Yes | Yes |
| Divider | Yes | Spacer+bg | Yes | Spacer |
| ForEach | Yes | forEach | Yes | forEach |
| Label | Yes | Row compound | Yes | Text fallback |
| Gauge | N/A | Text fallback | Yes | CircularProgressIndicator |
| Conditional | Yes | if | Yes | if |
| FamilySwitch | Yes | LocalSize | Yes | requestedSize |
| Config (AppIntent) | Yes | Config Activity | Yes (10+) | SharedPrefs |
| Native provider | Yes | JNI | Yes | JNI |
| sharedStorage | UserDefaults | SharedPrefs | UserDefaults | SharedPrefs |
| Deep linking (url) | widgetURL | clickable Intent | widgetURL | N/A |

## Platform-Specific Notes

### iOS
- Minimum deployment: iOS 17.0
- AppIntentConfiguration requires `import AppIntents`
- Widget extension memory limit: ~30MB

### Android
- Requires Glance dependency: `androidx.glance:glance-appwidget:1.1.0`
- Widget sizes mapped from iOS families: systemSmall=2x2, systemMedium=4x2, systemLarge=4x4
- `minimumScaleFactor` not supported in Glance (skipped with warning)

### watchOS
- Minimum deployment: watchOS 9.0
- Accessory families only (circular, rectangular, inline)
- Tighter memory (~15-20MB) and refresh budgets (hourly)
- AppIntent requires watchOS 10+; older versions get StaticConfiguration

### Wear OS
- Same native compilation as Android phone (Wear OS = Android)
- Requires Horologist + Tiles Material 3 dependencies
- Tiles are full-screen cards in the carousel
- `Gauge` maps to `CircularProgressIndicator`

## Build Instructions

### iOS
```bash
perry widget.ts --target ios-widget --app-bundle-id com.example.app -o widget_out
xcrun --sdk iphoneos swiftc -target arm64-apple-ios17.0 \
  widget_out/*.swift -framework WidgetKit -framework SwiftUI \
  -o widget_out/WidgetExtension
```

### Android
```bash
perry widget.ts --target android-widget --app-bundle-id com.example.app -o widget_out
# Copy .kt files to app/src/main/java/com/example/app/
# Copy xml/ to app/src/main/res/xml/
# Merge AndroidManifest_snippet.xml into AndroidManifest.xml
```

### watchOS
```bash
perry widget.ts --target watchos-widget --app-bundle-id com.example.app -o widget_out
xcrun --sdk watchos swiftc -target arm64-apple-watchos9.0 \
  widget_out/*.swift -framework WidgetKit -framework SwiftUI \
  -o widget_out/WidgetExtension
```

### Wear OS
```bash
perry widget.ts --target wearos-tile --app-bundle-id com.example.app -o widget_out
# Copy .kt files to Wear OS module
# Add Horologist + Tiles Material 3 dependencies to build.gradle
# Merge AndroidManifest_snippet.xml
```
