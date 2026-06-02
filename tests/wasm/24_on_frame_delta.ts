// onFrame on the web/wasm target must preserve deltaMs across the
// idiomatic self-rescheduling loop. The WASM bridge creates a fresh JS
// closure wrapper for each onFrame(loop) call, so runtime delta tracking
// must not be keyed by JS object identity.
import { onFrame } from "perry/ui";

let frames = 0;

function loop(_timestampMs: number, deltaMs: number): number {
  frames += 1;
  console.log("frame " + frames + " dt=" + deltaMs);
  if (frames < 3) {
    onFrame(loop);
  }
  return 0;
}

onFrame(loop);

setTimeout(() => {
  console.log("done");
}, 80);
