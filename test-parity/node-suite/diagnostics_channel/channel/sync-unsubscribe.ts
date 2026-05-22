import * as dc from "node:diagnostics_channel";

const name = "dc-sync-unsubscribe";
const events: string[] = [];
function first() {
  events.push("first");
  console.log("unsubscribe in publish:", dc.unsubscribe(name, first));
}
function second() {
  events.push("second");
}
dc.subscribe(name, first);
dc.subscribe(name, second);
dc.channel(name).publish("message");
console.log("events first publish:", events.join(","));
dc.channel(name).publish("message2");
console.log("events second publish:", events.join(","));
