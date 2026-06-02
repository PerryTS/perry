# Background Tasks

The `perry/background` module schedules deferred or periodic work that the
operating system runs even when the app is in the background — refreshing
data, polling for updates, or syncing state without keeping the app in the
foreground.

```typescript,no-test
import { registerTask, schedule, cancel } from "perry/background";

registerTask("com.example.refresh", async () => {
  await syncOrders();
});

schedule(
  "com.example.refresh",
  "appRefresh",
  Date.now() + 60_000,   // earliestStartMs
  true,                  // requiresNetwork
  false,                 // requiresCharging
);
```

## API

### `registerTask(identifier, handler)`

Register a handler for a background-task identifier. The OS calls this
handler when it decides to wake the app for the matching schedule.

- `identifier: string` — free-form, but on iOS / tvOS / visionOS it
  **must also appear in `Info.plist`** under
  `BGTaskSchedulerPermittedIdentifiers`. Apple rejects unregistered
  identifiers at submit time.
- `handler: () => Promise<void> | void` — async or sync. The OS gives a
  fixed budget (~30 s for `appRefresh`, several minutes for `processing`);
  Perry awaits the returned promise before signalling completion.

On iOS / tvOS, `registerTask` **must be called at module-init time**
(before the app loop starts). Perry's app delegate flushes the registry
during `application:didFinishLaunchingWithOptions:`. On Android,
visionOS, watchOS, and macOS the call can happen any time.

### `schedule(identifier, kind, earliestStartMs, requiresNetwork, requiresCharging)`

Submit a wake-up request for a registered identifier.

- `kind: "appRefresh" | "processing"`
  - `"appRefresh"` — short (~30 s) wake to refresh data. iOS:
    `BGAppRefreshTaskRequest`. Android: `OneTimeWorkRequest` with no
    power constraint.
  - `"processing"` — longer-running work that requires the device to
    meet `requiresNetwork` / `requiresCharging`. iOS:
    `BGProcessingTaskRequest`. Android: `OneTimeWorkRequest` with a
    matching `Constraints` builder.
- `earliestStartMs: number` — Unix-epoch milliseconds; pass `0` for "as
  soon as the OS allows".
- `requiresNetwork: boolean` — maps to
  `setRequiresNetworkConnectivity` (iOS/visionOS/tvOS),
  `setRequiredNetworkType(CONNECTED)` (Android), or
  `setRequiresNetworkConnectivity` on the macOS scheduler. Advisory on
  watchOS (the OS decides).
- `requiresCharging: boolean` — maps to `setRequiresExternalPower`
  (iOS/tvOS/visionOS), `setRequiresCharging(true)` (Android). Advisory on
  watchOS / macOS.

Calling `schedule` for an identifier that already has a pending request
**replaces it** — both iOS and Android enforce uniqueness per identifier.

### `cancel(identifier)`

Cancel a previously scheduled task. No-op for unknown ids. On watchOS
there is no native cancel API; `cancel` removes the handler from
Perry's registry so a fired refresh becomes a no-op.

## Platform support

| Platform | Backend | Wake while not running? |
|---|---|---|
| **iOS** | `BGTaskScheduler` | Yes (per Apple's policy) |
| **Android** | `androidx.work` (`OneTimeWorkRequest` + `PerryBackgroundWorker`) | Yes |
| **tvOS** | `BGTaskScheduler` (tvOS 13+) | Only while the box is on (during screensaver / different app) |
| **visionOS** | `BGTaskScheduler` (visionOS 1.0+) | Yes |
| **watchOS** | `WKApplication.scheduleBackgroundRefresh` (watchOS 7+) | Yes; only `appRefresh` kind, no native cancel |
| **macOS** | `NSBackgroundActivityScheduler` | Only while app is running |
| **GTK4 (Linux)** | No equivalent — silent no-op | — |
| **Windows** | No equivalent without admin or MSIX — silent no-op | — |
| **Web** | Silent no-op | — |

For Linux desktop and Win32 Perry apps, deploy-time scheduling
(`systemd --user` timer units, Windows Task Scheduler) is the only path;
the app cannot register them at runtime. For periodic refresh while a
desktop app is running, use `setInterval()` directly.

## iOS Info.plist requirement

iOS / tvOS / visionOS reject any `submitTaskRequest:` whose identifier
isn't whitelisted at compile time. Add the identifiers your app registers
to your `Info.plist`:

```xml
<key>BGTaskSchedulerPermittedIdentifiers</key>
<array>
  <string>com.example.refresh</string>
</array>
```

Without this entry the `submit` call fails silently and the OS never
delivers the wake-up.

## Android: Google's WorkManager

The Android implementation requires `androidx.work:work-runtime-ktx` on
the app's classpath. Perry's Android template already pulls it in —
`crates/perry-ui-android/template/app/build.gradle.kts`. If you ship a
custom Gradle setup, add:

```kotlin
implementation("androidx.work:work-runtime-ktx:2.9.0")
```

## Branching by platform

Use `getDeviceIdiom()` from `perry/system` to skip background scheduling
on platforms where it's a no-op:

```typescript,no-test
import { getDeviceIdiom } from "perry/system";
import { registerTask, schedule } from "perry/background";

const idiom = getDeviceIdiom();
if (idiom === "phone" || idiom === "pad" || idiom === "watch") {
  registerTask("refresh", refreshHandler);
  schedule("refresh", "appRefresh", 0, true, false);
} else {
  // Desktop fallback: poll while running
  setInterval(refreshHandler, 5 * 60 * 1000);
}
```

## Notes & limitations

- iOS budget is approximately 30 s for `appRefresh` and a few minutes
  for `processing` — design handlers around that.
- Android `WorkManager` enforces a 15-minute minimum for
  `PeriodicWorkRequest`; Perry's `schedule` always builds a
  `OneTimeWorkRequest` to avoid that constraint, but the OS may still
  delay the run based on doze mode and battery state.
- Promise-based completion is synchronous-best-effort: Perry pumps
  microtasks before and after invoking the handler, so simple `await`
  chains run, but a handler that returns a long-lived `Promise` may
  miss the OS's completion deadline.
