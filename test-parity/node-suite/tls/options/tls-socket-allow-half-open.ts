import tls from "node:tls";
import net from "node:net";
import { Duplex } from "node:stream";

const netBacked = new tls.TLSSocket(new net.Socket(), { allowHalfOpen: true });
console.log("net backed:", netBacked.allowHalfOpen);
netBacked.destroy();

const duplex = new Duplex({
  allowHalfOpen: false,
  read() {},
  write(_chunk, _encoding, callback) { callback(); },
});
const duplexBacked = new tls.TLSSocket(duplex, { allowHalfOpen: true });
console.log("duplex backed:", duplexBacked.allowHalfOpen);
duplexBacked.destroy();

const standalone = new tls.TLSSocket(undefined, { allowHalfOpen: true });
console.log("standalone:", standalone.allowHalfOpen);
standalone.destroy();
