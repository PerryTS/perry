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

/** Returns the device model identifier (e.g. "iPhone13,4"). */
export function getDeviceModel(): string;

/** Returns the BCP 47 locale tag for the device's primary language (e.g. "en-US", "fr-FR"). */
export function getLocale(): string;

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

// ---------------------------------------------------------------------------
// Audio input
// ---------------------------------------------------------------------------

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
