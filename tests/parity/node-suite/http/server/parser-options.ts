import { createServer, request } from "node:http";

const server = createServer({
  maxHeaderSize: 1024,
  joinDuplicateHeaders: true,
  requireHostHeader: false,
  rejectNonStandardBodyWrites: true,
  optimizeEmptyRequests: true,
  httpValidation: "relaxed",
} as any);
console.log(
  "server:",
  [
    server.maxHeaderSize,
    server.joinDuplicateHeaders,
    server.requireHostHeader,
    server.rejectNonStandardBodyWrites,
    server.httpValidation,
  ].join("|"),
);

const req = request({
  host: "example.test",
  agent: { addRequest() {} } as any,
  maxHeaderSize: 2048,
  insecureHTTPParser: true,
  joinDuplicateHeaders: true,
});
console.log(
  "request:",
  [req.maxHeaderSize, req.insecureHTTPParser, req.joinDuplicateHeaders].join(
    "|",
  ),
);
req.destroy();
