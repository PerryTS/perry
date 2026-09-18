import { EventEmitter } from "node:events";

// #10600: `class X extends EventEmitter` dispatches through
// perry-runtime's node_stream_event_emitter.rs, whose emit loop cloned
// listener callbacks (and the call args) into plain Rust locals, then
// called into user code that can allocate, then reused those unrooted
// copies for the next listener. A moving minor collection triggered by
// an allocation-heavy listener left the NEXT listener's snapshot entry
// pointing at retired from-space memory — no GC env knobs needed, this
// crashes under the plain default build.
class Bus extends EventEmitter {}

const bus = new Bus();
const seen: string[] = [];

bus.on("tick", function (this: any, tag: string) {
  seen.push("first:" + tag);
});

bus.on("tick", function (this: any, tag: string) {
  // Allocate enough that a moving minor collection lands here, while the
  // THIRD listener's closure is still a live, unrooted pointer captured
  // by the emit loop's listener snapshot.
  const churn: any[] = [];
  for (let i = 0; i < 6000; i++) {
    churn.push({ k: i, s: "x" + i, o: { i, s2: "y" + i } });
  }
  seen.push("second:" + tag + ":" + churn.length);
});

bus.on("tick", function (this: any, tag: string) {
  seen.push("third:" + tag);
});

for (let i = 0; i < 300; i++) {
  bus.emit("tick", "t" + i);
}

console.log(seen.length, seen[seen.length - 1]);
