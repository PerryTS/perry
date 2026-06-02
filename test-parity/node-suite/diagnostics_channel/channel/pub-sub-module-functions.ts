import { channel, subscribe, unsubscribe, hasSubscribers } from "node:diagnostics_channel";

const ch = channel("dc-pub-sub-module");
const events: string[] = [];
const input = { foo: "bar" };
function subscriber(message: any, name: string) {
  events.push(`${name}:${message.foo}`);
}

console.log("before:", hasSubscribers("dc-pub-sub-module"), ch.hasSubscribers);
subscribe("dc-pub-sub-module", subscriber);
console.log("after subscribe:", hasSubscribers("dc-pub-sub-module"), ch.hasSubscribers);
ch.publish(input);
console.log("events after publish:", events.join("|"));
console.log("unsubscribe found:", unsubscribe("dc-pub-sub-module", subscriber));
console.log("after unsubscribe:", hasSubscribers("dc-pub-sub-module"), ch.hasSubscribers);
ch.publish({ foo: "ignored" });
console.log("events final:", events.join("|"));
console.log("unsubscribe missing:", unsubscribe("dc-pub-sub-module", subscriber));
