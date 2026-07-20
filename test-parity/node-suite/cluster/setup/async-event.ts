// Node v26.5.0 setupPrimary emits one asynchronous `setup` event per call,
// carrying the exact settings snapshot from that call.
import cluster from "node:cluster";

const seen: string[] = [];
cluster.on("setup", (settings) => seen.push(`event:${settings.exec}`));
cluster.setupPrimary({ exec: "one" });
seen.push(`sync:${cluster.settings.exec}`);
cluster.setupMaster({ exec: "two" });
seen.push(`sync:${cluster.settings.exec}`);
setImmediate(() => console.log(seen.join("|")));
