import { channel } from "node:diagnostics_channel";

const ch = channel("dc-unsubscribe-during-publish");
const events: string[] = [];
function first() { events.push("first"); ch.unsubscribe(second); }
function second() { events.push("second"); }
ch.subscribe(first);
ch.subscribe(second);
ch.publish({});
ch.publish({});
console.log("events:", events.join(","));
