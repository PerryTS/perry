import http, { CloseEvent, MessageEvent, WebSocket } from "node:http";

console.log("types:", typeof CloseEvent, typeof MessageEvent, typeof WebSocket);
console.log(
  "global identities:",
  CloseEvent === globalThis.CloseEvent,
  MessageEvent === globalThis.MessageEvent,
  WebSocket === globalThis.WebSocket,
);
console.log(
  "default identities:",
  http.CloseEvent === CloseEvent,
  http.MessageEvent === MessageEvent,
  http.WebSocket === WebSocket,
);
console.log(
  "close prototype:",
  new CloseEvent("close", {
    code: 1000,
    reason: "ok",
    wasClean: true,
  }) instanceof Event,
);
console.log(
  "message prototype:",
  new MessageEvent("message", { data: "hello" }) instanceof Event,
);
