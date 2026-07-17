# System APIs Overview

The `perry/system` module provides access to platform-native system features:
preferences, secure storage, notifications, dark-mode detection, audio
capture, and app introspection. Every snippet below is excerpted from
[`docs/examples/system/snippets.ts`](../../examples/system/snippets.ts) — CI
links the file on every PR.

```typescript
{{#include ../../examples/system/snippets.ts:imports}}
```

## Available APIs

| Function | Description | Platforms |
|----------|------------|-----------|
| `openURL(url)` | Open URL in default browser/app | All |
| `isDarkMode()` | Check system dark mode | All |
| `getDeviceIdiom()` | `"phone"`, `"pad"`, `"mac"`, `"tv"`, … | All |
| `getDeviceModel()` | Device model identifier (e.g. `"iPhone13,4"`) | All |
| `preferencesSet(key, value)` | Store a preference (string or number) | All |
| `preferencesGet(key)` | Read a preference (returns `string | number | undefined`) | All |
| `keychainSave(key, value)` | Secure storage write | All |
| `keychainGet(key)` | Secure storage read | All |
| `keychainDelete(key)` | Secure storage remove | All |
| `notificationSend(title, body)` | Local notification | All |
| `notificationCancel(id)` | Cancel a scheduled notification | Apple, Android |
| `notificationOnTap(cb)` | Handle banner taps | Apple, Android |
| `notificationRegisterRemote(cb)` / `notificationOnReceive(cb)` | Push (APNs / FCM) | iOS, macOS; Android needs app-side Firebase setup — see [Notifications](notifications.md) |
| `audioStart()` / `audioStop()` | Microphone capture | All |
| `audioGetLevel()` / `audioGetPeak()` | RMS / peak amplitude (`0..1`) | All |
| `audioGetWaveform(n)` | Recent waveform samples for visualization | All |
| `audioSetOutputFilename(p)` / `audioStartRecording()` / `audioStopRecording()` | Capture mic to a WAV file | All native |
| `geolocationGetCurrent(ok, err)` | One-shot device position | iOS, Android, macOS |
| `geolocationWatch(cb)` / `geolocationStopWatch(id)` | Subscribe to position updates | iOS, Android, macOS |
| `geolocationRequestPermission(cb)` | Request location permission | iOS, Android, macOS |
| `imagePickerPick(max, multi, cb)` | Native photo-library picker | iOS, Android, macOS |
| `registerTask(id, fn)` / `schedule(id, …)` / `cancel(id)` | Deferred / periodic background work — see [`perry/background`](background.md) | iOS, Android, tvOS, visionOS, watchOS, macOS |

> **Clipboard** lives in `perry/ui` (not `perry/system`): import `clipboardRead`
> and `clipboardWrite` from there.

## Quick Example

```typescript
{{#include ../../examples/system/snippets.ts:dark-mode}}
```

```typescript
{{#include ../../examples/system/snippets.ts:preferences}}
```

```typescript
{{#include ../../examples/system/snippets.ts:open-url}}
```

## Next Steps

- [Preferences](preferences.md)
- [Keychain](keychain.md)
- [Notifications](notifications.md)
- [Audio Capture](audio.md)
- [Geolocation & Image Picker](geolocation.md)
- [Background Tasks](background.md)
- [Other](other.md)
