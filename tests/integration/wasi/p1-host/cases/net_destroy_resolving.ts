// Destroying a connection while its name is still being resolved cancels it:
// the host never connects, and the descriptor number is free for the next
// connection. The program outlives the cancelled lookup's 300 ms delay and uses
// the replacement connection after it, so a late answer for the old lookup would
// land on the reused descriptor.
import * as net from "node:net";

const started = Date.now();
const port = Number(process.env.PEER_PORT);
const first = net.connect(port, "slow.test");
first.on("error", (error: any) => console.log("first error", error.code));
first.on("close", () => console.log("first closed"));
setTimeout(() => {
  first.destroy();
  const second = net.connect(port, "127.0.0.1", () => {
    console.log("literal connected");
    setTimeout(() => second.write("ping"), 400);
  });
  second.on("data", (chunk) => {
    const late = Date.now() - started >= 350;
    console.log("got", String(chunk), late ? "after the lookup's deadline" : "too early");
    second.end();
  });
  second.on("error", (error: any) => console.log("literal error", error.code));
  second.on("close", () => console.log("literal closed"));
}, 50);
