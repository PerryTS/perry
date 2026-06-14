# Android

Perry compiles TypeScript apps for Android using JNI (Java Native Interface).

## Requirements

- Android NDK
- Android SDK
- Rust Android targets:
  ```bash
  rustup target add aarch64-linux-android armv7-linux-androideabi
  ```

## Building

```bash
perry app.ts -o app --target android
```

## UI Toolkit

Perry maps UI widgets to Android views via JNI:

| Perry Widget | Android Class |
|-------------|--------------|
| Text | TextView |
| Button | Button |
| TextField | EditText |
| SecureField | EditText (ES_PASSWORD) |
| Toggle | Switch |
| Slider | SeekBar |
| Picker | Spinner + ArrayAdapter |
| Image | ImageView |
| VStack | LinearLayout (vertical) |
| HStack | LinearLayout (horizontal) |
| ZStack | FrameLayout |
| ScrollView | ScrollView |
| Canvas | Canvas + Bitmap |
| NavigationStack | FrameLayout |

## Android-Specific APIs

- **Dark mode**: `Configuration.uiMode` detection
- **Preferences**: SharedPreferences
- **Keychain**: Android Keystore
- **Notifications**: NotificationManager
- **Open URL**: `Intent.ACTION_VIEW`
- **Alerts**: `PerryBridge.showAlert`
- **Sheets**: Dialog (modal)

## Splash Screen

Perry's Android template includes a splash theme (`Theme.Perry.Splash`) that displays a `windowBackground` drawable during cold start. Configure it via `perry.splash` in `package.json`:

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

The image is centered via a `layer-list` drawable with a solid background color. The activity switches to the normal theme in `onCreate` before inflating the layout, so the splash disappears as soon as the app is ready.

For full control, provide custom drawable and theme XML files:

```json
{
  "perry": {
    "splash": {
      "android": {
        "layout": "splash/splash_background.xml",
        "theme": "splash/themes.xml"
      }
    }
  }
}
```

See [Project Configuration](../getting-started/project-config.md#splash) for the full config reference.

## Differences from Desktop

- **Touch-only**: No hover events, no right-click context menus
- **Single window**: Multi-window maps to Dialog views
- **Toolbar**: Horizontal LinearLayout
- **Font**: Typeface-based font family support

## Next Steps

- [Wear OS](wearos.md) — full watch apps on the same Android backend
- [Platform Overview](overview.md) — All platforms
- [UI Overview](../ui/overview.md) — UI system
