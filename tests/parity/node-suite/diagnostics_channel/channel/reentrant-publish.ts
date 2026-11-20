import { channel } from "node:diagnostics_channel";

const ch = channel("dc-reentrant-publish-extra");
const events: string[] = [];
let depth = 0;
ch.subscribe(() => { events.push("sub" + depth); if (depth++ === 0) ch.publish({ nested: true }); depth--; });
ch.publish({});
console.log("events:", events.join(","));
