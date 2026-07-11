# Explicit Memory Control

JavaScript has no `free()`. For most programs that's the right default — Perry's
generational GC (see [Memory Model](memory-model.md)) decides when to collect.
But latency-sensitive programs (games, interactive UIs) and programs that churn
large binary buffers sometimes need to *choose the moment*: run the collection
between frames, not in the middle of one; release a 100 MB texture now, not at
the next full cycle. Perry gives you two standard-shaped tools for that.

## `ArrayBuffer.prototype.transfer()` (ES2024)

`transfer(newLength?)` moves an ArrayBuffer's contents into a new buffer and
**detaches** the source: its `byteLength` becomes 0, `detached` reports `true`,
views over it report length 0, and any further `transfer`/`slice`/view
construction on it throws a `TypeError`. This is standard ECMAScript — the same
code runs under Node and Bun unchanged.

```ts
let scratch = new ArrayBuffer(64 * 1024 * 1024);
// ... use it ...
scratch = scratch.transfer(0); // detach: the 64 MB backing is released
```

Perry gives detach real teeth: buffer bytes live inline in the GC heap, so the
runtime hands the page-aligned interior of a detached payload back to the OS
immediately (`madvise`). A large detached buffer stops costing RSS the moment
you transfer it, even while the (now empty) ArrayBuffer object is still
reachable. `transferToFixedLength()` behaves identically (Perry has no
resizable ArrayBuffers), and `structuredClone(v, { transfer: [...] })` detaches
through the same path.

## `perry/gc` — collection pacing

A Perry-native module in the spirit of `perry/thread`: it compiles to direct
runtime calls and does not resolve under Node/Bun, so guard the import if the
source must also run there.

```ts
import { collect, minor, idleHint } from "perry/gc";

collect();  // full collection now — same as the global gc()
minor();    // nursery-only collection now; returns freed bytes
idleHint(); // "this is a good moment": runs a collection only if one
            // is already due by the normal thresholds; returns whether
            // one ran. O(1) when nothing is due.
```

`idleHint()` is the one to reach for in a frame loop: call it once per frame
after presenting. When allocation pressure has made a collection imminent, it
runs at your chosen boundary instead of landing mid-frame at whatever
allocation happens to trip the threshold:

```ts
function frame() {
  update();
  render();
  idleHint(); // GC pause (if any) happens here, not inside update()
  requestFrame(frame);
}
```

## What Perry deliberately does NOT provide

A `free(value)` / `forget(value)` API. With a tracing GC, "free this object
now" is either equivalent to dropping the reference (which the compiler already
tracks) or it dangles every other reference to the object — a use-after-free
factory. The two mechanisms above cover the real use cases — bulk binary data
(`transfer`) and pause timing (`perry/gc`) — without making any correct program
crash.
