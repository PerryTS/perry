import { ClientRequest, OutgoingMessage } from "node:http";

const request = new ClientRequest({
  host: "example.test",
  agent: { addRequest() {} } as any,
  method: "PATCH",
  path: "/items?q=1",
});
request.on("error", () => {});

console.log(
  "instance:",
  request instanceof ClientRequest,
  request instanceof OutgoingMessage,
);
console.log(
  "request line:",
  request.method,
  request.path,
  request.protocol,
  request.host,
);
console.log("agent:", typeof request.agent?.addRequest);
console.log(
  "defaults:",
  request.aborted,
  request.destroyed,
  request.finished,
  request.reusedSocket,
);
console.log("max headers:", request.maxHeadersCount, request.maxHeaderSize);
request.destroy();
