import { channel, Channel } from "node:diagnostics_channel";

const ch = channel("dc-pub-sub-methods");
const events: string[] = [];
function sub(message: any, name: string) {
  events.push(`${name}:${message.count}`);
}

console.log("instance initial:", ch instanceof Channel);
console.log("initial:", ch.hasSubscribers);
ch.subscribe(sub);
console.log("subscribed:", ch.hasSubscribers, ch instanceof Channel);
ch.publish({ count: 1 });
console.log("events:", events.join(","));
console.log("unsubscribe found:", ch.unsubscribe(sub));
console.log("after unsubscribe:", ch.hasSubscribers);
console.log("unsubscribe missing:", ch.unsubscribe(sub));
