// BloomView embed demo (#5519) — render a live Bloom scene inside a Perry UI
// window on any platform Bloom supports (not just Windows).
//
// How it fits together:
//   * Perry UI owns the native window and the run loop (`App(...)`).
//   * `BloomView(w, h)` reserves a native render-surface view in the view tree
//     and sizes it (macOS/iOS/visionOS/tvOS via Auto Layout, GTK via
//     set_size_request, Android via a SurfaceView's LayoutParams, Windows via a
//     fixed-size child HWND).
//   * `bloomViewGetNativeHandle(view)` returns that view's platform handle
//     (NSView* / UIView* / GtkWidget* / ANativeWindow* / HWND).
//   * The Bloom engine attaches its GPU surface to that handle
//     (`attachToNSView` / `attachToUIView` / `attachToSurface`, all forwarding
//     to `bloom_attach_native`), then renders when the host drives frames.
//   * `onFrame(...)` drives the engine's frame loop from Perry UI's run loop.
//
// Build (macOS), with `@bloomengine/engine` resolvable (a `"bloom": "file:…"`
// dependency or `perry.compilePackages`):
//   perry examples/bloomview_embed_demo.ts -o demo && ./demo

import { App, VStack, BloomView, bloomViewGetNativeHandle, onFrame } from "perry/ui";
import { attachToNSView, beginDrawing, endDrawing, clearBackground, Colors } from "bloom/core";
import { drawCircle } from "bloom/shapes";

const WIDTH = 800;
const HEIGHT = 600;
const view = BloomView(WIDTH, HEIGHT);

let attached = false;
let t = 0;

function frame(_timestampMs: number, deltaMs: number): void {
  // The native view exists immediately, but its handle is only usable once the
  // window is on screen — attach on the first frame the handle is non-zero.
  if (!attached) {
    const handle = bloomViewGetNativeHandle(view);
    if (handle !== 0) {
      attached = attachToNSView(handle, WIDTH, HEIGHT);
    }
  }
  if (attached) {
    t = t + deltaMs * 0.001;
    beginDrawing();
    clearBackground(Colors.DARKBLUE);
    const x = WIDTH / 2 + Math.cos(t) * 220;
    const y = HEIGHT / 2 + Math.sin(t) * 140;
    drawCircle(x, y, 60, Colors.GOLD);
    endDrawing();
  }
  onFrame(frame); // re-arm for the next frame
}

onFrame(frame);
App({ title: "BloomView Embed (#5519)", width: WIDTH, height: HEIGHT, body: VStack([view]) });
