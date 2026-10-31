import { MessageChannel, MessagePort, receiveMessageOnPort } from "node:worker_threads";

const prototype = MessagePort.prototype;
const post = prototype.postMessage;
const first = new MessageChannel();
const second = new MessageChannel();
console.log("initial", Reflect.apply(prototype.hasRef, first.port1, []));
Reflect.apply(prototype.ref, first.port1, []);
console.log("ref", Reflect.apply(prototype.hasRef, first.port1, []));
Reflect.apply(prototype.unref, first.port1, []);
console.log("unref", Reflect.apply(prototype.hasRef, first.port1, []));
Reflect.apply(prototype.start, first.port1, []);
Reflect.apply(post, first.port1, ["prototype"]);
console.log("prototype", receiveMessageOnPort(first.port2)?.message);
const borrowed = first.port1.postMessage;
Reflect.apply(borrowed, second.port1, ["borrowed"]);
console.log("receiver", receiveMessageOnPort(second.port2)?.message);
console.log("owner", receiveMessageOnPort(first.port2) === undefined);
first.port1.postMessage("direct");
console.log("direct", receiveMessageOnPort(first.port2)?.message);

let effects = 0;
const fake = Object.create(MessagePort.prototype);
Object.defineProperty(fake, "__perryPortId", {
  get() { effects++; throw new Error("brand getter"); },
});
const proxy = new Proxy(first.port1, {
  get() { effects++; throw new Error("proxy getter"); },
});
const copied = { __perryPortId: (first.port1 as any).__perryPortId };
const invalid = [undefined, null, {}, fake, copied, proxy];
const payload = { get value() { effects++; return 1; } };
const options = { get transfer() { effects++; return []; } };
for (const name of ["postMessage", "start", "close", "ref", "unref", "hasRef"]) {
  let rejected = 0;
  for (const receiver of invalid) {
    try {
      Reflect.apply((prototype as any)[name], receiver, [payload, options]);
    } catch (error: any) {
      if (error.name === "TypeError" && error.code === undefined) rejected++;
    }
  }
  console.log(name, rejected);
}
console.log("effects", effects);
(first.port1 as any).__perryPortId = 99;
Reflect.apply(post, first.port1, ["mutated"]);
console.log("mutated", receiveMessageOnPort(first.port2)?.message);
delete (first.port1 as any).__perryPortId;
(first.port1 as any)["#<perry:private-value:0:@0:[[MessagePortId]]>"] = 55;
Reflect.apply(post, first.port1, ["private"]);
console.log("private", receiveMessageOnPort(first.port2)?.message);
delete (first.port1 as any)["#<perry:private-value:0:@0:[[MessagePortId]]>"];
Reflect.apply(prototype.close, first.port1, []);
Reflect.apply(prototype.close, first.port2, []);
console.log("closed", Reflect.apply(prototype.hasRef, first.port1, []));
Reflect.apply(prototype.start, first.port1, []);
Reflect.apply(post, first.port1, ["closed"]);
console.log("closed delivery", receiveMessageOnPort(first.port2) === undefined);
second.port1.close();
second.port2.close();
