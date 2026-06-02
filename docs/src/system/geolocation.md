# Geolocation & Image Picker

Two `perry/system` capabilities that wrap the OS's location and
photo-library pickers across iOS, Android, macOS, and stub on every
other platform.

## Geolocation

Callback-based; wrap in `new Promise(r => …)` at the call site if a
Promise-shaped API is preferred.

```typescript,no-test
import {
  geolocationGetCurrent,
  geolocationWatch,
  geolocationStopWatch,
  geolocationRequestPermission,
} from "perry/system";

geolocationGetCurrent(
  (lat, lng, accuracy, timestampMs) => {
    console.log(`at ${lat},${lng} ±${accuracy}m`);
  },
  (errorMessage) => {
    console.error("location failed:", errorMessage);
  },
);
```

### `geolocationGetCurrent(onSuccess, onError)`

Resolve the device's current position. Exactly one of the two callbacks
fires per invocation:

- `onSuccess(lat, lng, accuracy, timestampMs)` — `accuracy` in meters
  (horizontal); `timestampMs` is Unix epoch milliseconds.
- `onError(message)` — fires on permission denial, timeout, or platform
  unavailability. Common messages: `"permission-denied"`,
  `"no-location"`, `"no-provider-available"`,
  `"unsupported-platform"`.

### `geolocationWatch(callback): number`

Subscribe to position updates. Returns a numeric watch id; pass it to
`geolocationStopWatch` to cancel. Updates fire whenever the platform
reports movement greater than the OS's default distance filter.

### `geolocationStopWatch(id)`

Cancel a watch started by `geolocationWatch`. No-op on unknown ids.

### `geolocationRequestPermission(callback)`

Request location permission. Calls `callback(status)` where status is
one of `"granted"`, `"denied"`, `"restricted"`, or
`"unsupported-platform"`. Safe to call repeatedly — already-granted
permissions return immediately.

### Required configuration

| Platform | Configuration |
|---|---|
| **iOS** | `NSLocationWhenInUseUsageDescription` in `Info.plist`. Backed by `CLLocationManager`. |
| **Android** | `<uses-permission android:name="android.permission.ACCESS_FINE_LOCATION"/>` (or `ACCESS_COARSE_LOCATION`) in `AndroidManifest.xml`. Backed by `LocationManager`. |
| **macOS** | `NSLocationWhenInUseUsageDescription` in `Info.plist` for sandboxed apps. Backed by `CLLocationManager`. |
| **tvOS / watchOS / visionOS / GTK4 / Windows / Web** | No-op stub — `geolocationGetCurrent` invokes `onError` immediately with `"unsupported-platform"`. |

### Promise wrapper

```typescript,no-test
import { geolocationGetCurrent } from "perry/system";

function getPosition(): Promise<{
  lat: number;
  lng: number;
  accuracy: number;
  timestamp: number;
}> {
  return new Promise((resolve, reject) => {
    geolocationGetCurrent(
      (lat, lng, accuracy, timestamp) =>
        resolve({ lat, lng, accuracy, timestamp }),
      (msg) => reject(new Error(msg)),
    );
  });
}
```

## Image picker

Present the native photo-library picker. The callback receives an array
of absolute filesystem paths the user selected; read bytes via
`fs.readFileSync(path)` if needed.

```typescript,no-test
import { imagePickerPick } from "perry/system";

imagePickerPick(
  5,        // maxCount
  true,     // allowMultiple
  (paths) => {
    if (paths.length === 0) {
      console.log("user cancelled");
    } else {
      for (const p of paths) {
        console.log("picked:", p);
      }
    }
  },
);
```

### `imagePickerPick(maxCount, allowMultiple, callback)`

- `maxCount: number` — soft cap on selections. iOS Photo Picker enforces
  this when API supports; Android Photo Picker (API 33+) accepts a max
  in `[1, 10]`.
- `allowMultiple: boolean` — if `false`, only one image can be picked
  regardless of `maxCount`.
- `callback(paths: string[])` — fires once when the user dismisses the
  picker. `paths` is empty if the user cancelled.

### Platform implementations

| Platform | Backend | Permissions |
|---|---|---|
| **iOS** | `PHPickerViewController` | None — the system picker doesn't require Photos permission |
| **Android (API 33+)** | `MediaStore.ACTION_PICK_IMAGES` (Photo Picker) | None — privacy-preserving |
| **Android (API < 33)** | `ACTION_GET_CONTENT` fallback | `READ_MEDIA_IMAGES` (used only by the fallback path) |
| **macOS** | `NSOpenPanel` filtered to image UTIs | None |
| **All other targets** | No-op stub — `callback` invoked with `[]` immediately | — |

On Android, picked URIs are copied into the app's cache dir (named
`perry_pick_<ms>_<idx>.<ext>` with the extension inferred from the MIME
type) so the absolute path returned is safe to read with `fs`.

## Image compression

Pair the picker with the `sharp` package (compiled natively via Perry's
well-known bindings) to compress before upload:

```typescript,no-test
import sharp from "sharp";

const buf = await sharp(pickedPath)
  .resize({ width: 1600 })
  .jpeg({ quality: 80 })
  .toBuffer();
```

See [Other Modules](../stdlib/other.md#sharp) for the full `sharp`
surface.
