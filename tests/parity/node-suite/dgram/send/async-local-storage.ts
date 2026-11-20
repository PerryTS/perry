import { AsyncLocalStorage } from "node:async_hooks";
import * as dgram from "node:dgram";

const storage = new AsyncLocalStorage<string>();

await new Promise<void>((resolve, reject) => {
  storage.run("udp-context", () => {
    const receiver = dgram.createSocket("udp4");
    const sender = dgram.createSocket("udp4");
    receiver.once("error", reject);
    sender.once("error", reject);
    receiver.once("message", () => {
      console.log("message store:", storage.getStore());
      sender.close(() => receiver.close(resolve));
    });
    receiver.bind(0, "127.0.0.1", () => {
      sender.send("context", receiver.address().port, "127.0.0.1");
    });
  });
});
