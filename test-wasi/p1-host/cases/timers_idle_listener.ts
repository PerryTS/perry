// Timers keep firing while a listener nobody connects to sits idle. The host
// leaves the listener blocking until the guest says otherwise, so a runtime
// that adopts it without making it non-blocking stops here, inside accept.
import * as net from "node:net";

const server = net.createServer(() => {});
server.listen(0, () => {
  console.log("listening");
  let ticks = 0;
  const timer = setInterval(() => {
    console.log("tick", ++ticks);
    if (ticks === 5) {
      clearInterval(timer);
      server.close(() => console.log("closed"));
    }
  }, 20);
});
