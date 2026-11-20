import {
  Agent,
  IncomingMessage,
  OutgoingMessage,
  ServerResponse,
} from "node:http";

const cases: [string, Function, unknown[]][] = [
  ["agent destroy", Agent.prototype.destroy, []],
  ["agent keep alive", Agent.prototype.keepSocketAlive, [{}]],
  ["outgoing set header", OutgoingMessage.prototype.setHeader, [
    "X-Test",
    "one",
  ]],
  ["outgoing get header", OutgoingMessage.prototype.getHeader, ["X-Test"]],
  ["outgoing names", OutgoingMessage.prototype.getHeaderNames, []],
  ["outgoing flush", OutgoingMessage.prototype.flushHeaders, []],
  ["response write head", ServerResponse.prototype.writeHead, [200]],
  ["incoming timeout", IncomingMessage.prototype.setTimeout, [1]],
];

for (const [label, method, args] of cases) {
  try {
    method.call({}, ...args);
    console.log(label, "ok");
  } catch (error: any) {
    console.log(label, error.name, error.code);
  }
}
