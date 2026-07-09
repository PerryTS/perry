// Type declarations for perry/system — Perry's platform & system APIs
// These types are auto-written by `perry init` / `perry types` so IDEs
// and tsc can resolve `import { ... } from "perry/system"`.

// ---------------------------------------------------------------------------
// Theme & Device
// ---------------------------------------------------------------------------

/** Returns true if the system is in dark mode. */
export function isDarkMode(): boolean;

/** Returns the device idiom (e.g. "phone", "pad", "mac", "tv"). */
export function getDeviceIdiom(): string;

/** The device's safe-area insets in points (status bar / Dynamic Island,
 *  home indicator, system bars). Stable across rotation and device class. */
export interface SafeAreaInsets {
  top: number;
  right: number;
  bottom: number;
  left: number;
}

/**
 * Returns the current safe-area insets in points — the distances from each
 * screen edge that are obscured by system UI (status bar / Dynamic Island on
 * top, home indicator on bottom on iOS; the equivalent system bars on Android).
 * Returns all zeros on macOS / host. Use these instead of hardcoding magic
 * numbers (e.g. `59` for Dynamic Island, `34` for the home indicator) so a
 * custom top-of-screen or bottom-of-screen layout is correct on every device.
 *
 * ```ts
 * import { getSafeAreaInsets } from "perry/system";
 * const { top, right, bottom, left } = getSafeAreaInsets();
 * ```
 */
export function getSafeAreaInsets(): SafeAreaInsets;

/** Returns the device model identifier (e.g. "iPhone13,4"). */
export function getDeviceModel(): string;

/** Returns the BCP 47 locale tag for the device's primary language (e.g. "en-US", "fr-FR"). */
export function getLocale(): string;

/**
 * Returns a stable, human-readable OS-version string for the current
 * platform (e.g. `"15.2"`, `"macOS 14.5"`, `"Android 14"`). Common need
 * for crash-report and telemetry payloads; pairs with
 * `getDeviceModel()` / `getAppVersion()`.
 */
export function getOSVersion(): string;

/** Returns `perry.toml :: project.version` (e.g. "1.2.6"). */
export function getAppVersion(): string;

/** Returns `perry.toml :: project.build_number` as a number. */
export function getAppBuildNumber(): number;

/** Returns the effective app bundle identifier from `perry.toml`. */
export function getBundleId(): string;

/**
 * Returns a Widget handle rendering the application's icon at the given path,
 * or 0 on platforms where app-icon retrieval is not supported.
 *
 * On macOS/iOS the path is the `.icns` / `.png` asset path inside the app bundle.
 * Pass an empty string `""` to use the app bundle's default icon.
 */
export function getAppIcon(path: string): import("perry/ui").Widget;

// ---------------------------------------------------------------------------
// URL
// ---------------------------------------------------------------------------

/** Open a URL in the default browser or system handler. */
export function openURL(url: string): void;

// ---------------------------------------------------------------------------
// System share sheet (#917)
// ---------------------------------------------------------------------------

/**
 * Present the platform share sheet for plain text.
 *
 * Maps to `UIActivityViewController` on iOS, `NSSharingServicePicker`
 * on macOS, and `Intent.ACTION_SEND` on Android. The optional `title`
 * is the suggested subject/heading; pass `""` to omit. No-op on
 * platforms without a system share UI.
 */
export function shareText(text: string, title?: string): void;

/**
 * Present the platform share sheet for a URL. Identical surface to
 * `shareText` but the body is treated as a URL so the OS picks
 * URL-aware activities (Mail with a link, Messages with a preview,
 * etc.).
 */
export function shareUrl(url: string, title?: string): void;

// ---------------------------------------------------------------------------
// App Group / cross-process shared storage (#675)
// ---------------------------------------------------------------------------

/**
 * Write a value into the App Group's shared key-value store. Visible
 * to widget extensions, share extensions, watchOS targets, and other
 * processes belonging to the same App Group identifier. On Apple
 * platforms backed by `UserDefaults(suiteName:)`; other platforms
 * fall back to an in-process HashMap so the API surface is
 * exercisable in dev/tests (not actually cross-process there).
 */
export function appGroupSet(key: string, value: string): void;

/**
 * Read a value previously written via `appGroupSet`. Returns the
 * empty string when the key is absent.
 */
export function appGroupGet(key: string): string;

/** Remove a key from the App Group store. */
export function appGroupDelete(key: string): void;

// ---------------------------------------------------------------------------
// Keychain (secure credential storage)
// ---------------------------------------------------------------------------

/** Save a value to the system keychain. */
export function keychainSave(key: string, value: string): void;

/** Retrieve a value from the system keychain. */
export function keychainGet(key: string): string;

/** Delete a value from the system keychain. */
export function keychainDelete(key: string): void;

// ---------------------------------------------------------------------------
// User Preferences (persistent key-value storage)
// ---------------------------------------------------------------------------

/**
 * Read a preference value. Returns the stored string or number, or `undefined`
 * if the key is absent. The runtime branches on the NaN-box tag of the stored
 * NSUserDefaults entry, so callers see the original type back.
 */
export function preferencesGet(key: string): string | number | undefined;

/**
 * Write a preference value. Strings and numbers are stored natively via
 * NSUserDefaults / GSettings / the Windows registry depending on platform;
 * the same value round-trips through `preferencesGet`.
 */
export function preferencesSet(key: string, value: string | number): void;

// ---------------------------------------------------------------------------
// Haptics
// ---------------------------------------------------------------------------

/**
 * The haptic effect vocabulary understood by `hapticPlay`. Semantic types
 * (`"success"`, `"warning"`, `"error"`) map to the platform's notification
 * haptics; intensity types (`"light"`, `"medium"`, `"heavy"`) to impact
 * haptics; the rest to the closest platform equivalent (tick/click).
 */
export type HapticType =
  | "success"
  | "warning"
  | "error"
  | "light"
  | "medium"
  | "heavy"
  | "click"
  | "selection"
  | "directionUp"
  | "directionDown"
  | "start"
  | "stop";

/**
 * Play a haptic feedback effect. Maps to the platform's native haptic
 * engine: `WKInterfaceDevice.playHaptic:` on watchOS, `UIFeedbackGenerator`
 * on iOS, `VibrationEffect` on Android, `NSHapticFeedbackManager` (Force
 * Touch trackpad) on macOS, and `navigator.vibrate` on the Web. No-op on
 * platforms without a haptic engine (tvOS, visionOS, GTK4, Windows).
 *
 * ```ts
 * import { hapticPlay } from "perry/system";
 * hapticPlay("success"); // correct answer
 * hapticPlay("error");   // wrong answer (double buzz)
 * ```
 */
export function hapticPlay(type: HapticType): void;

// ---------------------------------------------------------------------------
// Notifications
// ---------------------------------------------------------------------------

/** Send a local notification. */
export function notificationSend(title: string, body: string): void;

/**
 * Register for remote (push) notifications.
 *
 * The callback fires once when the OS returns a device token. On Apple
 * platforms the token is formatted as the canonical uppercase hex string
 * (no spaces, no `<>`) that APNs-side code expects.
 *
 * Requires the relevant platform capability:
 * - iOS/macOS: APNs entitlement (`aps-environment`) + a provisioning profile.
 * - Android: Firebase Messaging + `google-services.json` (not yet wired).
 *
 * No-op on platforms without a push pipeline (tvOS, visionOS, watchOS, GTK4,
 * Windows, Web).
 */
export function notificationRegisterRemote(onToken: (token: string) => void): void;

/**
 * Register a handler for incoming remote-notification payloads received while
 * the app is foregrounded. The payload object is the APNs `aps` userInfo
 * dictionary (or equivalent platform shape) converted to a plain object.
 *
 * Background/terminated-app delivery uses `notificationOnBackgroundReceive`.
 */
export function notificationOnReceive(cb: (payload: object) => void): void;

/**
 * Register a handler for remote-notification payloads delivered while the app
 * is backgrounded or terminated (#98). The callback returns a `Promise<void>`
 * that signals when the work is finished — the OS hands us a fixed budget
 * (~30s on iOS) before the process is suspended; calling the OS completion
 * handler before the Promise resolves would cut that work off mid-flight.
 *
 * Backed by `application:didReceiveRemoteNotification:fetchCompletionHandler:`
 * on iOS and `FirebaseMessagingService.onMessageReceived` on Android. On both
 * platforms the OS completion signal is sent only after the returned Promise
 * resolves; if it rejects, the iOS path reports `UIBackgroundFetchResultFailed`
 * and the Android path logs the error.
 *
 * The Android pipeline currently requires the app's process to already be
 * loaded (the FCM service running inside the warm process). True cold-start
 * delivery (FCM waking a terminated app) is tracked as a #98 follow-up.
 *
 * No-op on macOS/tvOS/visionOS/watchOS/GTK4/Windows/Web — those platforms
 * have no equivalent background-delivery pipeline.
 */
export function notificationOnBackgroundReceive(
    cb: (payload: object) => Promise<void>,
): void;

/**
 * Schedule a local notification to fire on a trigger. The `id` lets you
 * cancel it later via `notificationCancel(id)`; scheduling a fresh trigger
 * with an existing id replaces the previous one (Apple-platform OS semantics).
 *
 * `trigger.type` must be a string literal at the call site so the codegen
 * can route to the right native trigger constructor:
 * - `"interval"` — fires after `seconds` (must be ≥ 60 if `repeats` is true,
 *    per UN constraints). Backed by `UNTimeIntervalNotificationTrigger` on
 *    Apple, `AlarmManager` on Android (not yet wired).
 * - `"calendar"` — fires once when wall-clock reaches `date`. Backed by
 *    `UNCalendarNotificationTrigger` on Apple.
 * - `"location"` — fires when the device enters the circular region. iOS-
 *    only via `UNLocationNotificationTrigger`; logged + skipped on macOS
 *    (no `CLLocationManager` notification trigger on the desktop OS).
 *
 * No-op on tvOS/visionOS/watchOS/GTK4/Windows/Web until the equivalent
 * native pipeline is wired.
 */
export function notificationSchedule(opts: {
    id: string;
    title: string;
    body: string;
    trigger:
        | { type: "interval"; seconds: number; repeats?: boolean }
        | { type: "calendar"; date: Date }
        | { type: "location"; latitude: number; longitude: number; radius: number };
}): void;

/**
 * Cancel a previously scheduled notification by id. No-op if no scheduled
 * notification with that id exists.
 */
export function notificationCancel(id: string): void;

/**
 * Register a handler for notification taps. Fires when the user taps the
 * notification banner (or selects an action button on platforms that wire
 * action support). The first arg is the notification id (the same string
 * passed to `notificationSchedule({id, …})` or — for `notificationSend` —
 * the per-platform default id).
 *
 * `action` is the action-button identifier when the user tapped a button,
 * or `undefined` for the default banner tap. Action-button registration
 * isn't wired yet; until it is, `action` is always `undefined`.
 *
 * Backed by `UNUserNotificationCenterDelegate.didReceiveNotificationResponse`
 * on Apple. No-op on tvOS/visionOS/watchOS/Android/GTK4/Windows/Web until
 * the equivalent native pipeline is wired.
 */
export function notificationOnTap(cb: (id: string, action?: string) => void): void;

// -----------------------------------------------------------------------------
// Audio input
// -----------------------------------------------------------------------------

/** Start audio capture. Returns 1 on success, 0 on failure. */
export function audioStart(): number;

/** Stop audio capture. */
export function audioStop(): void;

/** Get the current audio input level (0-1). */
export function audioGetLevel(): number;

/** Get the peak audio input level (0-1). */
export function audioGetPeak(): number;

/** Get waveform data with the given number of samples. */
export function audioGetWaveform(sampleCount: number): number;

/** Set the output filename for audio recording. */
export function audioSetOutputFilename(filename: string): void;

/** Start audio recording. */
export function audioStartRecording(): void;

/** Stop audio recording and save to file. */
export function audioStopRecording(): void;

/**
 * Register a callback for real-time audio sample processing.
 * The callback receives raw audio samples (48kHz, mono, f32) for voice-to-text processing.
 */
export function audioRegisterCallback(callback: (samplesPtr: number, numSamples: number) => void): void;

/** Unregister the audio callback. */
export function audioUnregisterCallback(): void;

// -----------------------------------------------------------------------------
// Geolocation (issue #552)
//
// Callback-based to keep the FFI surface flat. Wrap in `new Promise(r => ...)`
// at the call site if a Promise-shaped API is preferred.
//
// iOS:     CoreLocation / CLLocationManager. The app bundle MUST declare
//          NSLocationWhenInUseUsageDescription in Info.plist or the system
//          short-circuits the permission prompt.
// Android: LocationManager (GPS + NETWORK providers). Requires
//          ACCESS_FINE_LOCATION (or ACCESS_COARSE_LOCATION) in the manifest.
// macOS:   CoreLocation. Same Info.plist key as iOS for sandboxed apps.
// Other targets (tvOS / watchOS / visionOS / GTK4 / Windows / Web): no-op
//          stubs — `geolocationGetCurrent` invokes `onError` immediately with
//          `"unsupported-platform"`.
//
// Required manifest entries — the OS denies the permission silently when these
// are missing.
//
//   iOS Info.plist:
//     <key>NSLocationWhenInUseUsageDescription</key>
//     <string>Used to find items near you.</string>
//
//   Android AndroidManifest.xml (inside <manifest>, not <application>):
//     <uses-permission android:name="android.permission.ACCESS_FINE_LOCATION" />
//     <!-- Optional coarse fallback for users who deny precise: -->
//     <uses-permission android:name="android.permission.ACCESS_COARSE_LOCATION" />
//
// Photo-library picker (`imagePickerPick`) needs NO manifest entries: iOS
// PHPickerViewController is sandboxed and Android's ACTION_PICK_IMAGES /
// ACTION_GET_CONTENT both run out-of-process. Image compression (`sharp` from
// perry-stdlib / perry-ext-sharp) is pure userspace and also needs nothing.
// -----------------------------------------------------------------------------

/**
 * Resolve the device's current position. Calls `cb(lat, lng, accuracy, timestamp)`
 * once on success; calls `onError(message)` once on permission denial, timeout,
 * or platform unavailability. Exactly one of the two fires per invocation.
 *
 * `accuracy` is in meters (horizontal); `timestamp` is Unix epoch milliseconds.
 */
export function geolocationGetCurrent(
    onSuccess: (lat: number, lng: number, accuracy: number, timestamp: number) => void,
    onError: (message: string) => void,
): void;

/**
 * Subscribe to position updates. Returns a numeric watch id; pass it to
 * `geolocationStopWatch` to cancel. Updates fire whenever the platform reports
 * a movement greater than the OS's default distance filter.
 */
export function geolocationWatch(
    cb: (lat: number, lng: number, accuracy: number, timestamp: number) => void,
): number;

/** Cancel a watch started by `geolocationWatch`. No-op on unknown ids. */
export function geolocationStopWatch(id: number): void;

/**
 * Request location permission. Calls `cb(status)` where status is one of
 * `"granted"`, `"denied"`, `"restricted"`, or `"unsupported-platform"`.
 * Safe to call repeatedly — already-granted permissions return immediately.
 */
export function geolocationRequestPermission(
    cb: (status: string) => void,
): void;

// -----------------------------------------------------------------------------
// Photo-library image picker (issue #552)
//
// iOS:     PHPickerViewController (no Photos permission required).
// Android: ACTION_PICK_IMAGES (Photo Picker) on API 33+; ACTION_GET_CONTENT
//          fallback on older devices.
// macOS:   NSOpenPanel filtered to image UTIs.
// Other targets: no-op stubs that invoke `cb([])` immediately.
//
// The callback receives an array of absolute filesystem paths. Read bytes
// via `fs.readFileSync(path)` if needed.
// -----------------------------------------------------------------------------

/**
 * Present the native photo-library picker. `cb(paths)` fires once when the
 * user dismisses the picker. `paths` is empty if the user cancelled.
 */
export function imagePickerPick(
    maxCount: number,
    allowMultiple: boolean,
    cb: (paths: string[]) => void,
): void;

// -----------------------------------------------------------------------------
// In-app screen capture (issue #918)
//
// Capture the contents of the key window as a PNG and return a base64-encoded
// string. The returned value is the raw base64 payload (no `data:image/png;base64,`
// prefix) so callers can prepend their own scheme or write the decoded bytes
// to disk via `Buffer.from(s, "base64")` + `fs.writeFileSync`.
//
// Platform support:
//   - macOS:   CGWindowListCreateImage on the key NSWindow.
//   - iOS:     UIGraphics + drawViewHierarchyInRect on the key UIWindow.
//   - tvOS:    UIGraphics on the key UIWindow (same as iOS).
//   - visionOS: UIGraphics on the key UIWindow (same as iOS).
//   - GTK4:    GtkWidget snapshot of the active GtkWindow.
//   - Windows: GDI BitBlt of the active HWND, encoded with the `png` crate.
//   - Android: Canvas-backed Bitmap of the root View, JNI-driven (geisterhand
//              feature only — returns "" in plain builds).
//   - watchOS: not supported — returns "".
//   - CLI / headless builds with no UI host: returns "".
//
// The call is synchronous and runs on the calling thread. On macOS / iOS /
// tvOS / visionOS it must run on the main thread (the AppKit/UIKit thread
// the app was started on); calling from a background thread is undefined.
// -----------------------------------------------------------------------------

/**
 * Capture the key window as a PNG and return a base64-encoded string.
 * Returns `""` if no UI host is attached or capture fails on the platform.
 */
export function takeScreenshot(): string;

// -----------------------------------------------------------------------------
// Network reachability (issue #582)
//
// `networkGetStatus` invokes `cb` synchronously with the current connection
// state. `networkOnChange` subscribes to subsequent transitions; the returned
// id is passed back to `networkStopOnChange` to unsubscribe.
//
// `connectionType` is one of:
//   - "wifi"     — Wi-Fi
//   - "cellular" — mobile data (iOS / Android)
//   - "ethernet" — wired link (macOS / Android TV / desktop)
//   - "none"     — explicitly offline
//   - "unknown"  — connected but the OS didn't report a transport (or the
//                  monitor hasn't fired its first event yet)
//
// iOS:     NWPathMonitor (Network framework, iOS 12+).
// macOS:   NWPathMonitor (Network framework, 10.14+).
// Android: ConnectivityManager.registerDefaultNetworkCallback (API 24+).
//          Requires `<uses-permission android:name=
//          "android.permission.ACCESS_NETWORK_STATE" />` in
//          AndroidManifest.xml — the perry-ui-android template adds this
//          automatically.
// Other targets (tvOS / visionOS / watchOS / GTK4 / Windows): stub returns
// `(connected = true, kind = "unknown")` and `onChange` is a no-op (returns 0).
// -----------------------------------------------------------------------------

/**
 * Read the current network reachability state. The supplied callback fires
 * synchronously with `(connected, connectionType)`. If the platform monitor
 * hasn't observed its first event yet, `connected` may be `false` and
 * `connectionType` `"unknown"` until the first transition arrives.
 */
export function networkGetStatus(
    cb: (connected: boolean, connectionType: string) => void,
): void;

/**
 * Subscribe to network reachability change events. The callback fires every
 * time the OS reports a transition (typically Wi-Fi ↔ cellular, or
 * connected ↔ disconnected). Returns a numeric id; pass it to
 * `networkStopOnChange` to unsubscribe.
 */
export function networkOnChange(
    cb: (connected: boolean, connectionType: string) => void,
): number;

/** Cancel a subscription started by `networkOnChange`. No-op on unknown ids. */
export function networkStopOnChange(id: number): void;

// -----------------------------------------------------------------------------
// Deep links — Universal Links (iOS) / App Links (Android) / URL schemes (issue #583)
//
// Two URL families are unified behind a single callback:
//
//   1. Custom schemes: `myapp://chat/abc123`. Tapping such a link from a
//      notification, a website banner, or another app opens your app and
//      hands you the URL.
//   2. Universal / App Links: `https://yourdomain.com/reset?token=…`. These
//      open the app if installed (and fall through to the browser if not),
//      so the same link works in Mail / Messages / web.
//
// `source` is one of:
//   - `"cold-start"` — the URL was the launch URL (app was not running).
//   - `"foreground"` — the app was already running (or backgrounded) and
//     the OS handed us a URL.
//
// On iOS, both arms are wired:
//   - `application(_:open:options:)` / `scene(_:openURLContexts:)` for
//     custom schemes.
//   - `application(_:continue:restorationHandler:)` /
//     `scene(_:continueUserActivity:)` for Universal Links.
// On macOS the AppKit `application(_:open:)` and the `kAEGetURL` Apple
// Event handler cover both.
// On Android both arms route through `Activity.getIntent().getData()`
// (cold start) and `onNewIntent` (foreground).
//
// Required platform manifest entries — Perry generates these automatically
// from `package.json`'s `perry.deepLinks` section:
//
//   "perry": {
//     "deepLinks": {
//       "schemes": ["myapp"],
//       "universalLinks": {
//         "ios":     ["myapp.com", "www.myapp.com"],
//         "android": ["myapp.com", "www.myapp.com"]
//       }
//     }
//   }
//
//   - iOS: `CFBundleURLTypes` (custom scheme) + `com.apple.developer.
//          associated-domains` entitlement (`applinks:<host>`).
//   - Android: `<intent-filter android:autoVerify="true">` entries for
//          each scheme + host.
//
// Two server-side files YOU still own (Perry doesn't host them — they
// live on the domain you declare in `universalLinks`):
//
//   - https://yourdomain.com/.well-known/apple-app-site-association
//     (Apple App Site Association — JSON, signed via your team ID +
//     bundle ID, served with `Content-Type: application/json`).
//   - https://yourdomain.com/.well-known/assetlinks.json
//     (Android Asset Links — JSON, lists the SHA-256 fingerprint of your
//     APK signing certificate).
//
// Without those files on the host, iOS / Android refuse to associate the
// domain with the app and the link falls through to the browser. Once
// they're in place, `appOnOpenUrl` fires.
//
// Other targets (tvOS / visionOS / watchOS / GTK4 / Windows / Web): the
// platform's URL surface is either nonexistent or different (Windows Toast
// activations, Web window.location). Stubs never invoke the callback;
// `appGetLaunchUrl()` returns `""`.
// -----------------------------------------------------------------------------

/**
 * Register the deep-link handler. The callback fires when the OS hands us a
 * URL — whether at launch, while running, or when transitioning from the
 * background. Setting a fresh handler replaces the previous one.
 *
 * If the app was launched by a deep link (`source = "cold-start"`), the
 * callback fires once on registration with the launch URL — so registering
 * the handler at module load time is enough; no separate `appGetLaunchUrl`
 * read is required for the cold-start flow.
 */
export function appOnOpenUrl(
    cb: (url: string, source: "cold-start" | "foreground") => void,
): void;

/**
 * Read the URL that launched the app, if any. Returns `""` when the app was
 * launched normally (icon tap, system, dock). Useful when the cold-start
 * flow needs to read the URL synchronously before the first frame —
 * otherwise prefer `appOnOpenUrl`, which delivers the same URL through the
 * unified callback path.
 */
export function appGetLaunchUrl(): string;
