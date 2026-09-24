// #11196: perry-stdlib's common handle registry (StringDecoder, crypto
// hashes, AsyncLocalStorage, the bundled EventEmitter, ...) minted ids from
// its own counter, while every `perry-ext-*` wrapper (net sockets and
// servers among them) minted from perry-ffi's. Both counters started at 1 in
// the same band, so a stdlib object and a net socket could share one numeric
// id, and whichever registry was consulted first answered for the other:
// `decoder.write()` ran `socket.write()` and returned `false`, and
// `events.once(socket, 'connect')` parked its promise on an unrelated
// EventEmitter and never settled — the redis@6.1.0 `connect()` hang.
import * as net from "node:net";
import * as events from "node:events";
import { createHash } from "node:crypto";
import { StringDecoder } from "node:string_decoder";
import { AsyncLocalStorage } from "node:async_hooks";

const emitterCtor: any = (events as any).EventEmitter;
const server: any = net.createServer((s: any) => {
  s.end("pong");
});
const client: any = new net.Socket();

// Stdlib-registry objects created AFTER two ext-net handles, so on the broken
// build they received the same numeric ids as `server` and `client`.
const hash: any = createHash("sha256");
const emitter: any = new emitterCtor();
const decoder: any = new StringDecoder("utf8");
const als: any = new AsyncLocalStorage();

hash.update("abc");
console.log("hash:", hash.digest("hex").slice(0, 16));
console.log("decoder.write:", JSON.stringify(decoder.write(Buffer.from("hé", "utf8"))));
console.log("decoder.end:", JSON.stringify(decoder.end()));
console.log("als.run:", als.run(42, () => als.getStore()));
let emitted = 0;
emitter.on("connect", () => emitted++);
console.log("emitter.emit:", emitter.emit("connect"), "count:", emitted);
console.log("server.listening:", server.listening);
console.log("client.connecting:", client.connecting, "destroyed:", client.destroyed);

server.listen(0, "127.0.0.1", async () => {
  const port = server.address().port;
  client.connect(port, "127.0.0.1");
  // The redis call shape: an indirect call through the namespace object.
  const connected = (0, events.once)(client, "connect");
  console.log("connect listeners after once:", client.listenerCount("connect"));
  await connected;
  console.log("once(client, connect) resolved");
  let data = "";
  client.on("data", (d: any) => {
    data += d;
  });
  await (0, events.once)(client, "end");
  console.log("client received:", data);
  console.log("emitter listeners after the socket traffic:", emitter.listenerCount("connect"));
  client.destroy();
  server.close(() => console.log("server closed"));
});
