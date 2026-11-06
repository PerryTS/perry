import { boundedChannel, BoundedChannel, channel } from "node:diagnostics_channel";

const bounded = boundedChannel("dc-bounded-run");
const events: string[] = [];
const handlers = {
  start(message: any) {
    events.push(`start:${message.value}`);
  },
  end(message: any) {
    events.push(`end:${message.value}`);
  },
};

console.log("boundedChannel typeof:", typeof boundedChannel);
console.log("BoundedChannel typeof:", typeof BoundedChannel);
console.log("instanceof BoundedChannel:", bounded instanceof BoundedChannel);
console.log("start channel identity:", bounded.start === channel("tracing:dc-bounded-run:start"));
console.log("end channel identity:", bounded.end === channel("tracing:dc-bounded-run:end"));
console.log("initial hasSubscribers:", bounded.hasSubscribers);
console.log("methods:", [typeof bounded.subscribe, typeof bounded.unsubscribe, typeof bounded.run, typeof bounded.withScope].join(","));
console.log("subscribe return:", bounded.subscribe(handlers));
console.log("after subscribe:", bounded.hasSubscribers);

const thisArg = { marker: "thisArg" };
const result = bounded.run({ value: 7 }, function (this: any, a: string, b: string) {
  events.push(`fn:${this === thisArg}:${a}:${b}`);
  return 42;
}, thisArg, "a", "b");

{
  using scope = bounded.withScope({ value: 9 });
  events.push(`scope:${bounded.hasSubscribers}`);
}

console.log("run result:", result);
console.log("events:", events.join("|"));
console.log("unsubscribe return:", bounded.unsubscribe(handlers));
console.log("after unsubscribe:", bounded.hasSubscribers);
