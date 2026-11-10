import { channel, hasSubscribers } from "node:diagnostics_channel";

const name = Symbol("dc-symbol");
const ch = channel(name);
const events: string[] = [];
ch.subscribe((message: any, receivedName: symbol) => {
  events.push(`${receivedName === name}:${message.value}`);
});
console.log("name is symbol:", typeof ch.name);
console.log("initial global hasSubscribers:", hasSubscribers(name));
ch.publish({ value: "ok" });
console.log("events:", events.join("|"));
console.log("after publish hasSubscribers:", hasSubscribers(name));
