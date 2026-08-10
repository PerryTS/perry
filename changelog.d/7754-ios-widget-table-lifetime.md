### Fixed

- **iOS: the widget table no longer retains every widget ever created, and child removal is no longer quadratic.** `register_widget` (`perry-ui-ios/src/widgets/mod.rs`) only ever pushed. Nothing removed. `remove_child` unparented a view with `removeFromSuperview`, but the table's `Retained<UIView>` kept it allocated for the lifetime of the process — so a list that rebuilds leaked its entire history of rows.

  On top of that, `clear_children` recovered handles for the subviews it was removing by **linear-scanning the whole table, once per subview**. That is O(children × every widget ever created), against a vector that only grew: k rebuilds of an N-row list cost O(k²N²) in total and got measurably worse the longer the app ran. Any list benchmark was measuring this rather than the framework.

  Two changes:

  - A `HANDLE_BY_PTR` reverse index makes handle recovery O(1), replacing the scan.
  - `clear_children` and `remove_child` now release the removed subtree. Release is **recursive**: a list row is normally a container with children, and freeing only the directly-removed child would strand its descendants — each still holds a table entry, and that entry is a strong reference, so the subtree stays alive behind a parent nobody can reach.

  **Slots are tombstoned, never reused.** Handles are handed to TypeScript as plain NaN-boxed numbers and must stay inside the `< 0x100000` handle band (`HANDLE_BAND_MAX`, with Web Stream ids immediately above at `[0x100000, 0x200000)`), which leaves no spare bits for the generation counter that safe slot reuse would require. Without one, a recycled index lets a stale handle silently drive a *different* widget. A tombstone costs one `Option` niche and turns a stale handle into `None` — the honest answer for a handle whose widget is gone.

  Worth noting the old behaviour was also a latent *correctness* bug, not only a memory one: a long-running app that created more than ~1M widgets would have walked its handles out of the handle band and into the stream-id range.

  **Behaviour change:** using a handle after its widget has been removed is now a no-op instead of driving a still-retained view. `widgetRemoveChild` is documented as removal and the layout API has a separate `widgetReorderChild` for moves, so remove-then-re-add was not a supported re-parenting idiom.

  `perry-ui-ios` is `#![cfg(target_os = "ios")]`, so none of this is reachable from a host unit test and it carries no CI coverage; it was verified on-device.

### Added

- **`PERRY_*` environment variables now reach an app launched on a simulator or device.** A bundled app is launched by the system rather than inherited from the shell, and `perry run` passed no environment at all — so `PERRY_FRAME_STATS=1 perry run --device …` silently collected nothing. Simulator launches forward via `SIMCTL_CHILD_*`; device launches build a JSON object for `devicectl --environment-variables` (via `serde_json`, so a value containing a quote cannot produce a malformed argument). Scoped to the `PERRY_` prefix deliberately — forwarding the whole environment would push the developer's `PATH`, `HOME`, credentials and locale into a sandboxed process that has its own.

- **`high_refresh_rate` in `perry.toml`** (`[ios]`, falling back to `[project]`) emits `CADisableMinimumFrameDurationOnPhone` into the generated Info.plist. Without that key iOS caps an iPhone at 60 Hz regardless of what `CADisplayLink` requests, so ProMotion hardware reports a hard 60 — which reads as "the framework cannot sustain 120" rather than "the bundle never opted in". iPad ProMotion has no such gate. Off by default: uncapped frame rates cost battery, and that is the app author's call rather than the compiler's. Covered by 5 unit tests.

- **Frame stats report periodically** (`PERRY_FRAME_STATS_INTERVAL`, default 300 frames; `0` disables). An iOS app has no exit at which to print a summary and the metrics are not reachable from TypeScript, so a device run would otherwise collect numbers nobody could see. Each line covers the frames since the previous one, which is what makes "p99 while I was scrolling" a meaningful reading. The interval is also runtime-settable via `js_frame_metrics_set_report_interval`.

- **`benchmarks/ios-ui/`** — a frame-time benchmark app cycling through idle, property-updates, add/remove, animation and text-heavy phases, printing a marker before each so `[frame-stats]` lines are attributable to a workload.
