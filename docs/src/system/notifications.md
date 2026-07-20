# Notifications

Send local notifications using the platform's notification system. Every
snippet below is excerpted from
[`docs/examples/system/snippets.ts`](../../examples/system/snippets.ts) — CI
links it on every PR.

## Sending a notification

```typescript
{{#include ../../examples/system/snippets.ts:notification-send}}
```

## Reacting to a tap

```typescript
{{#include ../../examples/system/snippets.ts:notification-tap}}
```

`action` is the action-button identifier when the user picks a button, or
`undefined` for the default banner tap.

## Cancelling a scheduled notification

```typescript
{{#include ../../examples/system/snippets.ts:notification-cancel}}
```

`notificationCancel(id)` is a no-op if no scheduled notification with that id
exists.

## Push notifications (APNs / Firebase)

```typescript
{{#include ../../examples/system/snippets.ts:notification-remote}}
```

`notificationRegisterRemote(cb)` fires once when the OS returns a device token
— on Apple platforms the token is the canonical uppercase hex string APNs
expects. `notificationOnReceive(cb)` runs whenever a remote payload arrives
while the app is foregrounded; the payload is the APNs `aps` userInfo
dictionary (or equivalent platform shape) converted to a plain object.

Requires the relevant platform capability. On iOS/macOS that's the APNs
entitlement (below). On Android the scaffolded app ships **without**
Firebase — there is no `google-services.json` — so
`notificationRegisterRemote` logs a warning describing the setup it needs
and returns without doing anything. To enable it, add Firebase Messaging to
the generated Android project (the `com.google.gms.google-services` Gradle
plugin, the `com.google.firebase:firebase-messaging` dependency, and your
`google-services.json`), then forward your `FirebaseMessagingService`'s
`onNewToken` / `onMessageReceived` to `PerryBridge.nativeNotificationToken`
/ `nativeNotificationReceive` (and/or
`nativeNotificationBackgroundReceive`) — the native side of those callbacks
is already wired ([#95](https://github.com/PerryTS/perry/issues/95),
[#98](https://github.com/PerryTS/perry/issues/98)).
No-op on platforms without a push pipeline (tvOS, visionOS, watchOS, GTK4,
Windows, Web).

### Enabling APNs on iOS

`registerForRemoteNotifications` only succeeds when the signed `.app` carries
the `aps-environment` entitlement. Opt in from `perry.toml`
([#5074](https://github.com/PerryTS/perry/issues/5074)):

```toml
[ios]
push_notifications = true          # emit the aps-environment entitlement
# push_environment = "production"  # default "development"; set for distribution
```

With this set, `perry compile --target ios` writes `aps-environment` into the
bundle's `app.entitlements` (defaulting to `development`, which matches
dev-signed builds), and `perry setup ios` / `perry run --target ios` enable the
Push Notifications capability on the App ID when minting the development
provisioning profile. For App Store / Ad Hoc distribution set
`push_environment = "production"`.

## Local notifications on Android

Local and scheduled notifications work out of the box on a freshly
scaffolded Android app:

- `notificationSend(title, body)` posts immediately to the `perry_default`
  notification channel (created on demand, API 26+). Like the Apple
  implementations it uses the fixed id `"perry_notification"`, so
  `notificationCancel("perry_notification")` removes it.
- `notificationSchedule(...)` interval/calendar triggers arm an
  `AlarmManager` alarm that fires a broadcast receiver, so the banner is
  delivered even if the process has since exited. Timing is *inexact*
  (exact alarms need the `SCHEDULE_EXACT_ALARM` special permission on
  Android 12+), and repeating intervals under 60 seconds are clamped to 60
  seconds by the OS. Location triggers are not wired
  ([#96](https://github.com/PerryTS/perry/issues/96) follow-up).
- `notificationCancel(id)` cancels the pending alarm *and* removes an
  already-delivered banner with that id from the shade.
- `notificationOnTap(cb)` fires while the app process is alive (the tap
  intent routes through `PerryActivity`). A tap that cold-starts the
  process is logged and skipped — no JS callback can be registered before
  the app has run.
- **Permission**: Android 13+ requires the `POST_NOTIFICATIONS` runtime
  grant. The template declares it and `PerryActivity` requests it (with the
  other dangerous permissions) at first launch; if the user denies it,
  notification calls log a warning to logcat and drop the banner instead of
  crashing.

## Platform Implementation

| Platform | Backend |
|----------|---------|
| macOS | UNUserNotificationCenter |
| iOS | UNUserNotificationCenter |
| Android | NotificationManager + AlarmManager (local/scheduled); FCM push requires app-side Firebase setup |
| Windows | Toast notifications |
| Linux | GNotification |
| Web | Web Notification API |

> **Permissions**: On macOS, iOS, Android 13+, and Web, the user may need to
> grant notification permissions. On first use (app launch on Android), the
> system will prompt automatically.

## Next Steps

- [Keychain](keychain.md) — Secure storage
- [Other](other.md) — Additional system APIs
- [Overview](overview.md) — All system APIs
