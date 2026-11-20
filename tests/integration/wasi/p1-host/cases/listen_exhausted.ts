// With one listener handed over, a second listen() has nothing to claim.
import * as net from "node:net";

const first = net.createServer(() => {});
const second = net.createServer(() => {});
first.listen(0, () => {
  console.log("first listening");
  second.on("error", (error: any) => {
    console.log("second", error.code);
    first.close();
  });
  second.listen(0, () => console.log("second listening unexpectedly"));
});
