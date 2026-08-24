# Frame Callbacks (`onFrame`)

`onFrame` subscribes a callback to the next display-link "tick". Use it for
time-based rendering — animations driven from code, simulations, games,
real-time data visualizations, or custom `Canvas` transitions — where you
need a frame-aligned tick with an accurate timestamp instead of
`setInterval(cb, 16)`.

```typescript,no-test
import { onFrame, cancelFrame } from "perry/ui";

function loop(timestampMs: number, deltaMs: number) {
  // advance simulation, redraw...
  onFrame(loop); // schedule the next frame
}

const id = onFrame(loop);
// later, to stop:
cancelFrame(id);
```

## Semantics

- **One-shot.** The callback fires *once*. To keep a loop running, call
  `onFrame` again from inside the callback (this mirrors the web's
  idiomatic `requestAnimationFrame` shape and avoids the "how do I stop a
  recurring callback" footgun).
- **`timestampMs`** is monotonic time since app start, in milliseconds,
  double precision.
- **`deltaMs`** is the time since the previous fire of *this* callback (0
  on the first call). Tracking is keyed off the callback identity so the
  idiomatic `onFrame(loop)` pattern gets accurate deltas without the app
  bookkeeping anything.
- **Order.** Subscribers fire in registration order each frame.
- **Pause when invisible.** The web backend uses `requestAnimationFrame`,
  which is paused automatically when the tab is hidden. iOS drives a real
  `CADisplayLink`, which the system stops while the app is suspended. The
  remaining native backends drive frames from their main-loop pump; treat
  that as a soft guarantee for now, with per-platform display-link drivers
  as a follow-up.

## Platform mapping

| Platform | Driver |
|---|---|
| Web (WASM) | `requestAnimationFrame` |
| iOS | **`CADisplayLink`** (vsync-aligned, `NSRunLoopCommonModes`) |
| macOS | Main-thread pump (CADisplayLink wiring TBD) |
| tvOS / visionOS | Main-thread pump (CADisplayLink wiring TBD) |
| Android | Main-thread pump (Choreographer wiring TBD) |
| GTK4 (Linux) | Main-loop pump (`gtk_widget_add_tick_callback` TBD) |
| Windows | WM_TIMER pump (DwmFlush vsync wiring TBD) |

On iOS, `timestampMs` comes from the display link and carries sub-microsecond
resolution. On the platforms still on the main-thread pump it comes from
`js_timer_now()`, which is truncated to whole milliseconds — fine for driving
an animation, too coarse to measure one.

## Frame-time metrics

The display-link driver also feeds a frame-time recorder, so frame pacing can
be measured on-device rather than inferred. It is off by default; set
`PERRY_FRAME_STATS=1` to collect.

Collection reports **p50/p95/p99 frame times, the longest frame, and a
dropped-frame count** over a rolling window of the most recent 8192 frames.
Dropped frames are derived from the display's own nominal frame duration, so
the same numbers are correct at 60 Hz and 120 Hz without configuration.

A frame is counted as dropped when the interval to the previous vsync spans
more than one nominal frame — jitter within half a frame of nominal is not a
drop, and a doubled interval is exactly one.

```
[frame-stats] frames=1840 budget=8.33ms p50=8.34ms p95=9.10ms p99=16.71ms max=41.20ms dropped=12
```

Because collection keeps the display link awake, an app with no `onFrame`
subscribers — a plain scrolling list, say — still produces a full frame trace.

### Measuring at 120 Hz

When metrics are enabled the link requests the top of the ProMotion range
instead of the adaptive default, so a benchmark measures the framework rather
than ProMotion throttling a static screen.

**On iPhone this is capped at 60 Hz unless the app's `Info.plist` contains
`CADisableMinimumFrameDurationOnPhone = YES`.** Perry's generated plist does
not currently emit that key, so 120 Hz measurement on iPhone needs it added
first; iPad ProMotion has no such opt-in.

### Interpreting a run

`js_frame_metrics_report()` returns the number of samples it summarized.
Zero means the display link never ticked — distinguishing "every frame was
fast" from "nothing was measured", which otherwise look identical in a
summary line.

Deliberate pauses (backgrounding, an explicit stop/start) are excluded from
the distribution rather than recorded as one enormous frame. An app suspended
*without* the driver noticing will still show a single long interval, so treat
a lone multi-second `max` as a lifecycle artifact rather than a stutter.
