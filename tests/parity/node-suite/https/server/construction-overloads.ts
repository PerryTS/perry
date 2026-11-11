import * as https from "node:https";

const listener = () => {};
const noArguments = https.createServer();
const optionsOnly = https.createServer({});
const listenerOnly = https.createServer(listener);
const requestListeners = listenerOnly.listeners?.("request") ?? [];
console.log(
  "instances:",
  noArguments instanceof https.Server,
  optionsOnly instanceof https.Server,
  listenerOnly instanceof https.Server,
);
console.log(
  "listeners:",
  noArguments.listenerCount?.("request"),
  optionsOnly.listenerCount?.("request"),
  requestListeners[0] === listener,
);
noArguments.close();
optionsOnly.close();
listenerOnly.close();
