// A TLS socket destroyed while its host name resolves closes in the
// close-callbacks phase, after the ticks and immediates queued before it.
import * as tls from "node:tls";

function round(label: string, error: Error | undefined, next: () => void) {
  const socket = tls.connect(1, "localhost");
  socket.on("error", (e: any) => console.log(label, "error", e.message));
  socket.on("close", (hadError) => {
    console.log(label, "close", hadError);
    next();
  });
  socket.destroy(error);
  console.log(label, "destroyed", socket.destroyed);
  process.nextTick(() => console.log(label, "tick"));
  setImmediate(() => console.log(label, "immediate"));
}

round("plain", undefined, () => round("error", new Error("boom"), () => {}));
