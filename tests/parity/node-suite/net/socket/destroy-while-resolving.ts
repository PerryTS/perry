// A socket destroyed while its name is resolving already owns a handle in
// Node, so its 'close' comes from the close-callbacks phase: after the ticks
// and the immediates queued before it, and before the next iteration's timers.
import * as net from "node:net";

function resolving(label: string, next: () => void): net.Socket {
  const socket = net.connect(1, "localhost");
  socket.on("error", (error: any) => console.log(label, "error", error.message));
  socket.on("close", (hadError) => {
    console.log(label, "close", hadError, socket.destroyed);
    next();
  });
  return socket;
}

function fromScript(label: string, error: Error | undefined, next: () => void) {
  const socket = resolving(label, next);
  socket.destroy(error);
  console.log(label, "destroyed", socket.destroyed);
  process.nextTick(() => console.log(label, "tick"));
  setImmediate(() => console.log(label, "immediate"));
}

function fromImmediate() {
  setImmediate(() => {
    resolving("in-immediate", () => {}).destroy();
    setTimeout(() => console.log("in-immediate", "timeout"), 0);
  });
  setImmediate(() => console.log("in-immediate", "sibling immediate"));
}

fromScript("plain", undefined, () =>
  fromScript("error", new Error("boom"), fromImmediate),
);
