import * as dgram from "node:dgram";

function codeOf(fn) {
  try {
    fn();
    return "none";
  } catch (error) {
    return error.code;
  }
}

function closeSocket(socket) {
  return new Promise((resolve) => {
    let events = 0;
    let callbacks = 0;
    function done() {
      if (events === 1 && callbacks === 1) {
        resolve();
      }
    }
    socket.once("close", () => {
      events += 1;
      done();
    });
    socket.close(() => {
      callbacks += 1;
      done();
    });
  });
}

const server = dgram.createSocket("udp4");
let listeningEvents = 0;
server.on("listening", () => {
  listeningEvents += 1;
});

await new Promise((resolve) => {
  server.bind(0, "127.0.0.1", () => resolve());
});

const addr = server.address();
console.log("bind address:", addr.address, addr.family, typeof addr.port, addr.port > 0);
console.log("listening events:", listeningEvents);

const client = dgram.createSocket("udp4");
let messageText = "";
let rinfoSummary = "";
let sendErr = "unset";
let sendBytes = -1;

await new Promise((resolve) => {
  let gotMessage = false;
  let gotSend = false;
  function done() {
    if (gotMessage && gotSend) {
      resolve();
    }
  }
  server.once("message", (msg, rinfo) => {
    messageText = msg.toString();
    rinfoSummary = `${rinfo.address}|${rinfo.family}|${typeof rinfo.port}|${rinfo.size}`;
    gotMessage = true;
    done();
  });
  client.send(Buffer.from("hello udp"), addr.port, "127.0.0.1", (err, bytes) => {
    sendErr = err === null ? "null" : err.code;
    sendBytes = bytes;
    gotSend = true;
    done();
  });
});

console.log("message:", messageText);
console.log("rinfo:", rinfoSummary);
console.log("send callback:", sendErr, sendBytes);

const connected = dgram.createSocket("udp4");
let connectEvents = 0;
connected.on("connect", () => {
  connectEvents += 1;
});

await new Promise((resolve) => {
  connected.connect(addr.port, "127.0.0.1", () => resolve());
});

const remote = connected.remoteAddress();
let connectedMessage = "";
await new Promise((resolve) => {
  server.once("message", (msg) => {
    connectedMessage = msg.toString();
    resolve();
  });
  connected.send("connected payload");
});

console.log("connect events:", connectEvents);
console.log("remote address:", remote.address, remote.family, remote.port === addr.port);
console.log("connected message:", connectedMessage);
console.log("disconnect result:", connected.disconnect());
console.log("remote after disconnect:", codeOf(() => connected.remoteAddress()));
console.log("disconnect after disconnect:", codeOf(() => connected.disconnect()));

console.log("bad type:", codeOf(() => dgram.createSocket("udp9")));
const unbound = dgram.createSocket("udp4");
console.log("address before bind:", codeOf(() => unbound.address()));
unbound.close();
console.log("bad msg:", codeOf(() => client.send(123, addr.port, "127.0.0.1")));
console.log("bad port:", codeOf(() => client.send(Buffer.from("x"), 70000)));

await closeSocket(client);
await closeSocket(connected);
await closeSocket(server);
console.log("closed");
