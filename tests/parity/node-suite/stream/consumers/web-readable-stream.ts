import { ReadableStream } from "node:stream/web";
import { bytes, text } from "node:stream/consumers";

const forText = new ReadableStream({
  start(controller) {
    controller.enqueue("we");
    controller.enqueue("b");
    controller.close();
  },
});
console.log("web text:", await text(forText as any));

const forBytes = new ReadableStream({
  start(controller) {
    controller.enqueue(new Uint8Array([65]));
    controller.enqueue(new Uint8Array([66]));
    controller.close();
  },
});
console.log("web bytes:", Array.from(await bytes(forBytes as any)).join(","));
