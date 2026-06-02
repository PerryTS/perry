// Type declarations for perry/background — deferred / periodic background work
// (issue #538). Maps to BGTaskScheduler on iOS and WorkManager on Android.
//
// iOS:     `BGTaskScheduler.shared`. The identifier passed to `registerTask`
//          MUST also be listed in Info.plist under
//          `BGTaskSchedulerPermittedIdentifiers`. `registerTask` itself
//          MUST be invoked at module-init time, BEFORE the app's run-loop
//          starts — Perry's app delegate flushes the handler registry
//          during `application:didFinishLaunchingWithOptions:`, and Apple
//          rejects late registrations. Background time budget is ~30 s
//          for `appRefresh`, several minutes for `processing`.
// Android: `androidx.work:work-runtime`. Backed by `OneTimeWorkRequest` +
//          a `PerryBackgroundWorker` whose `doWork` calls back into the
//          registered handler. The `processing` kind maps to the same
//          `OneTimeWorkRequest` shape with `requiresNetwork` /
//          `requiresCharging` constraints applied via `Constraints`.
// tvOS:    `BGTaskScheduler` (tvOS 13+). Same surface as iOS. Practical
//          caveat: tvOS apps only run while the box is on; "background"
//          here means "while a different app is active" or "during the
//          screen-saver", not while the box is sleeping.
// visionOS: `BGTaskScheduler` (visionOS 1.0+). Same surface as iOS.
//          Registration is flushed from Perry's `app_run` before the
//          SwiftUI host takes over UIApplicationMain.
// watchOS: `WKApplication.scheduleBackgroundRefresh(...)` (watchOS 7+).
//          Only the `appRefresh` kind has a watchOS equivalent;
//          `processing` is accepted but treated identically.
//          `requiresNetwork` / `requiresCharging` are advisory — the OS
//          decides scheduling based on its own conditions. There is no
//          native cancel API on watchOS; `cancel(id)` removes the
//          handler so a fired refresh becomes a no-op.
// macOS:   `NSBackgroundActivityScheduler` (10.10+). Different model from
//          iOS — the scheduler fires while the app is running, with
//          `interval`/`tolerance` derived from `earliestStartMs`.
// GTK4 (Linux) and Windows: NO real impl — desktop OSes don't expose an
//          app-managed "wake up while not running" pipeline that doesn't
//          require admin elevation (Win Task Scheduler) or deploy-time
//          configuration (systemd timer / MSIX background-task manifest).
//          `registerTask`, `schedule`, and `cancel` are silent no-ops on
//          these targets. For periodic refresh while the app IS running,
//          use `setInterval()` directly.
// Web:     no-op stubs.

/** Async or sync handler invoked when the OS wakes the app for a registered
 *  task. The OS gives Perry a fixed budget (~30 s on iOS appRefresh;
 *  ~10 min on Android WorkManager); the wake-up's completion signal fires
 *  after the returned Promise resolves. */
export type BackgroundTaskHandler = () => Promise<void> | void;

/**
 * Register a handler for a background-task identifier. On iOS this MUST be
 * called during module initialization (before the Perry app loop starts);
 * on Android the call can be made at any time before the matching `schedule`.
 *
 * The `identifier` is a free-form string, but on iOS it must also appear in
 * the app's `Info.plist` under `BGTaskSchedulerPermittedIdentifiers` — Apple
 * rejects unregistered identifiers at scheduling time.
 */
export function registerTask(
    identifier: string,
    handler: BackgroundTaskHandler,
): void;

/**
 * Schedule a registered task to run.
 *
 * - `kind: "appRefresh"` — short (~30 s) wake to refresh data. iOS:
 *   `BGAppRefreshTaskRequest`. Android: `OneTimeWorkRequest` with no power
 *   constraint. Use this for opportunistic syncs (the spec's "polling
 *   fallback" case in issue #538).
 * - `kind: "processing"` — longer-running work that requires the device to
 *   meet the supplied constraints. iOS: `BGProcessingTaskRequest`. Android:
 *   `OneTimeWorkRequest` with matching `Constraints`.
 *
 * `earliestStartMs` is a Unix-epoch millisecond timestamp; pass `0` for
 * "as soon as the OS allows". `requiresNetwork` / `requiresCharging` map
 * to `requiresNetworkConnectivity` / `requiresExternalPower` on iOS and to
 * the matching `Constraints` builder methods on Android.
 *
 * Calling `schedule` for an identifier that already has a pending request
 * replaces it — both platforms enforce uniqueness per identifier.
 *
 * Returns void (synchronous enqueue on both platforms; resolution of the
 * actual work happens later through the registered handler).
 */
export function schedule(
    identifier: string,
    kind: "appRefresh" | "processing",
    earliestStartMs: number,
    requiresNetwork: boolean,
    requiresCharging: boolean,
): void;

/** Cancel a previously scheduled task by identifier. No-op on unknown ids. */
export function cancel(identifier: string): void;
