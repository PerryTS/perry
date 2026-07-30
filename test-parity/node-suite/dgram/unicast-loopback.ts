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
const client = dgram.createSocket("udp4");
const connected = dgram.createSocket("udp4");
const unbound = dgram.createSocket("udp4");
let unboundClosed = false;
let listeningEvents = 0;
const onListening = () => {
  listeningEvents += 1;
};
server.on("listening", onListening);
let connectEvents = 0;
const onConnect = () => {
  connectEvents += 1;
};
connected.on("connect", onConnect);

try {
  await new Promise((resolve) => {
    server.bind(0, "127.0.0.1", () => resolve());
  });

  const addr = server.address();
  console.log(
    "bind address:",
    addr.address,
    addr.family,
    typeof addr.port,
    addr.port > 0,
  );
  console.log("listening events:", listeningEvents);

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
      rinfoSummary = `${rinfo.address}|${rinfo.family}|${typeof rinfo
        .port}|${rinfo.size}`;
      gotMessage = true;
      done();
    });
    client.send(
      Buffer.from("hello udp"),
      addr.port,
      "127.0.0.1",
      (err, bytes) => {
        sendErr = err === null ? "null" : err.code;
        sendBytes = bytes;
        gotSend = true;
        done();
      },
    );
  });

  console.log("message:", messageText);
  console.log("rinfo:", rinfoSummary);
  console.log("send callback:", sendErr, sendBytes);

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
  console.log(
    "remote address:",
    remote.address,
    remote.family,
    remote.port === addr.port,
  );
  console.log("connected message:", connectedMessage);
  console.log("disconnect result:", connected.disconnect());
  console.log(
    "remote after disconnect:",
    codeOf(() => connected.remoteAddress()),
  );
  console.log(
    "disconnect after disconnect:",
    codeOf(() => connected.disconnect()),
  );

  console.log("bad type:", codeOf(() => dgram.createSocket("udp9")));
  console.log("address before bind:", codeOf(() => unbound.address()));
  await new Promise((resolve) => unbound.close(resolve));
  unboundClosed = true;
  console.log(
    "bad msg:",
    codeOf(() => client.send(123, addr.port, "127.0.0.1")),
  );
  console.log("bad port:", codeOf(() => client.send(Buffer.from("x"), 70000)));
} finally {
  if (!unboundClosed) await closeSocket(unbound);
  await Promise.all([
    closeSocket(client),
    closeSocket(connected),
    closeSocket(server),
  ]);
  server.removeListener("listening", onListening);
  connected.removeListener("connect", onConnect);
}
console.log("closed");
