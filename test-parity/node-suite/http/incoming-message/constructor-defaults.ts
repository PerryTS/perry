import { IncomingMessage } from "node:http";

const message = new IncomingMessage(null as any);
console.log(
  "meta:",
  message.method,
  message.url,
  message.statusCode,
  message.statusMessage,
);
console.log(
  "version:",
  message.httpVersion,
  message.httpVersionMajor,
  message.httpVersionMinor,
);
console.log("state:", message.complete, message.aborted, message.destroyed);
console.log(
  "headers:",
  Object.keys(message.headers).length,
  Object.keys(message.headersDistinct).length,
  message.rawHeaders.length,
);
console.log(
  "trailers:",
  Object.keys(message.trailers).length,
  Object.keys(message.trailersDistinct).length,
  message.rawTrailers.length,
);
console.log("socket:", message.socket, message.connection);
message.destroy();
