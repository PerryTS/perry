### Added

- **iOS: `onFrame` is driven by a real `CADisplayLink`, and frame times are measurable.** `perry/ui`'s frame callbacks were previously pumped from the 8 ms repeating `NSTimer` in `perry-ui-ios/src/app.rs` via `js_frame_pump_default()`. That is wrong for anything frame-shaped in two independent ways, and together they made iOS frame pacing unmeasurable rather than merely imprecise.

  **Not vsync-aligned.** An 8 ms free-running `NSTimer` beats against a 8.333 ms (120 Hz) or 16.667 ms (60 Hz) refresh instead of landing on it, and is subject to run-loop coalescing. **Millisecond-quantized.** `js_timer_now()` is `elapsed().as_millis() as f64` — whole milliseconds, or ~12% of a 120 Hz frame budget. A p99 computed from that clock is noise.

  New `perry-ui-ios/src/frame_driver.rs` installs a `CADisplayLink` on the main run loop and drives `js_frame_tick` from it. The timer pump keeps driving `setTimeout`/`setInterval`, microtasks, the stdlib pump and the GC step; it no longer drives frames, and now only reconciles the link's paused state.

  Three details that are load-bearing rather than incidental:

  - **`NSRunLoopCommonModes`, not the default mode.** A `UIScrollView` drag switches the run loop to `UITrackingRunLoopMode`. A link registered only in the default mode goes silent for the entire gesture — exactly the window a scrolling benchmark exists to measure.
  - **The link's timestamp is rebased, not passed through.** `CADisplayLink.timestamp` is a `CFTimeInterval` on the `CACurrentMediaTime` base (seconds since boot), while `onFrame`'s documented contract is monotonic milliseconds since app start. Handing the raw media time to JS would silently break every app doing `t - startTime`. The first tick pins the media clock against `js_timer_now()` and later ticks report an offset from that pin, keeping the documented epoch while carrying the link's sub-microsecond resolution.
  - **Pause state is polled from the timer pump, not from the link.** A paused link receives no ticks, so it cannot observe a newly-registered `onFrame` callback and wake itself.

- **Frame-time metrics (`PERRY_FRAME_STATS=1`).** New `perry-runtime/src/frame_metrics.rs` turns a display-link vsync stream into p50/p95/p99 frame times, longest frame, and a dropped-frame count over a rolling 8192-frame window. Off by default; `js_frame_metrics_set_enabled` flips it at runtime so a benchmark can scope collection to one workload without relaunching.

  Dropped frames are derived from the driver-supplied *nominal* frame duration (`CADisplayLink.duration`) rather than a hardcoded 60 Hz, which is what makes the same code correct on a 120 Hz ProMotion display: `dropped = round(interval / expected) - 1`, so jitter within half a frame is not a drop and a doubled interval is exactly one.

  Three cases that would otherwise produce confident wrong numbers are handled explicitly:

  - **Deliberate pauses are not 4-second frames.** Backgrounding stops the link; attributing the gap as an interval would report hundreds of phantom dropped frames and poison the p99. `js_frame_metrics_mark_discontinuity` drops the running baseline without discarding collected samples, and the driver calls it on every pause transition.
  - **One hitch cannot dominate the drop count.** A genuine 10 s stall is recorded at full length as the longest frame, but its dropped-frame attribution is capped, so the count still describes the run rather than the stall.
  - **`js_frame_metrics_report()` returns its sample count.** Zero distinguishes "every frame was fast" from "the display link never ticked" — the two are indistinguishable in a summary line, and the second is the failure mode where a probe reports success without its subject having run.

  Collection keeps the link awake on its own, so an app with **no** `onFrame` subscribers — a plain scrolling list — still produces a full frame trace.

  Covered by 11 unit tests in `perry-runtime --lib` (nearest-rank percentiles, steady 120 Hz, the doubled-interval case, sub-half-frame jitter, the attribution cap, discontinuity, non-monotonic driver timestamps, ring-buffer wrap, the disabled path, and the report-zero case). They are unit tests rather than an integration suite deliberately: per-PR CI runs `--lib --bins`, so coverage placed under `crates/*/tests/` would not gate this.

  **Known limitation, on-device 120 Hz.** With metrics enabled the link requests the top of the ProMotion range instead of the adaptive default. On iPhone that request is capped at 60 Hz unless the bundle sets `CADisableMinimumFrameDurationOnPhone = YES`, and Perry's generated `Info.plist` does not currently emit that key — so iPhone 120 Hz measurement needs that added first. iPad ProMotion has no such opt-in. Backgrounding an app *without* a pause transition the driver observes will still show one long interval; treat a lone multi-second `max` as a lifecycle artifact.

  macOS, tvOS, visionOS, Android, GTK4 and Windows still drive `onFrame` from their main-loop pumps and are unchanged; `docs/src/ui/on-frame.md` now states per-platform which clock backs `timestampMs`.
