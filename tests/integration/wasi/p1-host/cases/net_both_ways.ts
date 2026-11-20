// The same program built both ways. By default the module imports only
// wasi_snapshot_preview1 and an outbound connection is not supported; with
// PERRY_WASM_NET=1 it imports perry_net and connects.
import * as net from "node:net";

const socket = net.connect(Number(process.env.PEER_PORT), "127.0.0.1");
socket.on("connect", () => {
  console.log("connected");
  socket.end();
});
socket.on("error", (error: any) => console.log("error", error.code));
