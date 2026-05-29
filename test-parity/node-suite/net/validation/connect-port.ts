import * as net from "node:net";

// #2013: socket.connect(port) / net.connect({ port }) reject a numeric port
// outside [0, 65536) with RangeError [ERR_SOCKET_BAD_PORT]. The message is
// prefixed "Port" (vs. "options.port" for listen).
const badPorts: any[] = [-1, 65536, 70000, NaN, 2.5];

for (const port of badPorts) {
  try {
    const sock = new net.Socket();
    sock.connect(port, "127.0.0.1");
    console.log("connect", String(port), "=> NO THROW");
  } catch (err: any) {
    console.log("connect", String(port), "=>", err.name, err.code, "|", err.message);
  }
}

// The net.connect({ port }) options-object overload validates the port the
// same way (RangeError [ERR_SOCKET_BAD_PORT], "Port" prefix).
for (const port of [-1, 99999] as any[]) {
  try {
    net.connect({ port, host: "127.0.0.1" });
    console.log("connect({port})", String(port), "=> NO THROW");
  } catch (err: any) {
    console.log("connect({port})", String(port), "=>", err.name, err.code, "|", err.message);
  }
}
