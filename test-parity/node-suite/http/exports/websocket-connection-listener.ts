// #2566/#2700: recent/legacy node:http export surface. Node exposes
// `WebSocket` as the global WebSocket constructor and `_connectionListener`
// as an enumerable function export.
import httpDefault, * as http from "node:http";
import { WebSocket, _connectionListener } from "node:http";

const keys = Object.keys(httpDefault as any);

console.log("keys:", keys.includes("WebSocket"), keys.includes("_connectionListener"));
console.log(
  "connectionListener:",
  typeof _connectionListener,
  (_connectionListener as any).length,
  (_connectionListener as any).name,
  (httpDefault as any)._connectionListener === _connectionListener,
  (http as any)._connectionListener === _connectionListener,
);
console.log(
  "WebSocket:",
  typeof WebSocket,
  (httpDefault as any).WebSocket === WebSocket,
  WebSocket === globalThis.WebSocket,
);
console.log(
  "WebSocket constants:",
  (WebSocket as any).CONNECTING,
  (WebSocket as any).OPEN,
  (WebSocket as any).CLOSING,
  (WebSocket as any).CLOSED,
);
console.log(
  "enumerable:",
  Object.getOwnPropertyDescriptor(httpDefault as any, "WebSocket")?.enumerable,
  Object.getOwnPropertyDescriptor(httpDefault as any, "_connectionListener")?.enumerable,
);
console.log(
  "prototype methods:",
  typeof (WebSocket as any).prototype.send,
  typeof (WebSocket as any).prototype.close,
);
