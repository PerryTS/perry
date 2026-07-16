import { MessageChannel } from "node:worker_threads";

const { port1, port2 } = new MessageChannel();
const expected = new Error("iterator-boom");

try {
  port1.postMessage({}, {
    transfer: {
      *[Symbol.iterator]() {
        throw expected;
      },
    },
  } as any);
  console.log("post: ok");
} catch (error: any) {
  console.log("same error:", error === expected, error?.message);
}

port1.close();
port2.close();
