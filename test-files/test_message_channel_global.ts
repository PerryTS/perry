// #4873: bare `new MessageChannel()` as a global constructor must link
// standalone (no worker_threads import anywhere in the graph) and produce a
// real { port1, port2 } object — React's scheduler feature-detects exactly
// this way at module init.
const c = new MessageChannel();
console.log(typeof c, typeof c.port1, typeof c.port2);

// The scheduler-shaped feature detection branch.
if (typeof MessageChannel !== "undefined") {
  const channel = new MessageChannel();
  const port = channel.port2;
  console.log("scheduler-branch", typeof port.postMessage);
}

// globalThis member form.
const g = new globalThis.MessageChannel();
console.log("globalThis-form", typeof g.port1, typeof g.port2);

// BroadcastChannel rides the same lowering.
const bc = new BroadcastChannel("chan");
console.log("broadcast", typeof bc, bc.name);
