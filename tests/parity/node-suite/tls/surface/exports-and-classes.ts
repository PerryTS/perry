import tls from "node:tls";
import { EventEmitter } from "node:events";
import { Duplex } from "node:stream";

const functions = [
  "checkServerIdentity", "connect", "createSecureContext", "createServer",
  "getCACertificates", "getCiphers", "setDefaultCACertificates",
];
console.log("functions:", functions.every((name) => typeof (tls as any)[name] === "function"));
console.log("classes:", typeof tls.SecureContext === "function", typeof tls.Server === "function", typeof tls.TLSSocket === "function");
console.log("inheritance:", tls.Server.prototype instanceof EventEmitter, tls.TLSSocket.prototype instanceof Duplex);
console.log("aliases:", tls.createServer !== tls.Server, tls.createSecureContext !== tls.SecureContext);
console.log("constants:", tls.DEFAULT_MIN_VERSION, tls.DEFAULT_MAX_VERSION, tls.CLIENT_RENEG_LIMIT, tls.CLIENT_RENEG_WINDOW);
