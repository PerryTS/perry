import {
  Agent,
  ClientRequest,
  IncomingMessage,
  OutgoingMessage,
  Server,
  ServerResponse,
} from "node:http";
import { EventEmitter } from "node:events";
import { Readable } from "node:stream";
import { Server as NetServer } from "node:net";

console.log(
  "names:",
  [
    Agent,
    ClientRequest,
    IncomingMessage,
    OutgoingMessage,
    Server,
    ServerResponse,
  ].map((value) => value.name).join("|"),
);
console.log("agent emitter:", Agent.prototype instanceof EventEmitter);
console.log(
  "incoming readable:",
  IncomingMessage.prototype instanceof Readable,
);
console.log(
  "client outgoing:",
  ClientRequest.prototype instanceof OutgoingMessage,
);
console.log(
  "response outgoing:",
  ServerResponse.prototype instanceof OutgoingMessage,
);
console.log("server net:", Server.prototype instanceof NetServer);
console.log(
  "constructor links:",
  [
    Agent,
    ClientRequest,
    IncomingMessage,
    OutgoingMessage,
    Server,
    ServerResponse,
  ].every((value) => value.prototype.constructor === value),
);
