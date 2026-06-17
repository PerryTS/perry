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
  which is paused automatically when the tab is hidden. The native
  backends drive frames from their main-loop pump; treat that as a soft
  guarantee for now and a real per-platform display-link driver is a
  follow-up.

## Platform mapping

| Platform | Driver |
|---|---|
| Web (WASM) | `requestAnimationFrame` |
| macOS | Main-thread pump (CADisplayLink wiring TBD) |
| iOS / tvOS / visionOS | Main-thread pump (CADisplayLink wiring TBD) |
| Android | Main-thread pump (Choreographer wiring TBD) |
| GTK4 (Linux) | Main-loop pump (`gtk_widget_add_tick_callback` TBD) |
| Windows | WM_TIMER pump (DwmFlush vsync wiring TBD) |
