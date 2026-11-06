// #793: `http.OutgoingMessage` is an exported constructor with the shared
// outgoing-message header and writable-state surface.
import * as http from "node:http";

const OutgoingMessage = http.OutgoingMessage as any;

console.log("export typeof:", typeof OutgoingMessage);
console.log("keys include:", Object.keys(http).includes("OutgoingMessage"));
console.log("ctor length/name:", OutgoingMessage.length, OutgoingMessage.name);

const om = new OutgoingMessage();
console.log("instance typeof:", typeof om);
console.log(
  "methods:",
  [
    typeof om.addTrailers,
    typeof om.appendHeader,
    typeof om.setHeaders,
    typeof om.cork,
    typeof om.uncork,
    typeof om.destroy,
    typeof om.end,
    typeof om.flushHeaders,
    typeof om.pipe,
    typeof om.setTimeout,
    typeof om.write,
    typeof om.setHeader,
    typeof om.getHeader,
    typeof om.hasHeader,
    typeof om.removeHeader,
    typeof om.getHeaderNames,
    typeof om.getHeaders,
    typeof om.on,
    typeof om.addListener,
  ].join("|"),
);
console.log(
  "properties:",
  [
    typeof om.connection,
    om.connection === null,
    typeof om.headersSent,
    om.headersSent,
    typeof om.socket,
    om.socket === null,
    typeof om.writableCorked,
    om.writableCorked,
    typeof om.writableEnded,
    om.writableEnded,
    typeof om.writableFinished,
    om.writableFinished,
    typeof om.writableHighWaterMark,
    om.writableHighWaterMark,
    typeof om.writableLength,
    om.writableLength,
    typeof om.writableObjectMode,
    om.writableObjectMode,
  ].join("|"),
);
console.log(
  "response-only methods:",
  typeof om.writeHead,
  typeof om.writeContinue,
  typeof om.writeEarlyHints,
  typeof om.writeProcessing,
);
