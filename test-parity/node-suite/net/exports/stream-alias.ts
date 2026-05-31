import * as net from "node:net";

const keys = Object.keys(net);

console.log("keys includes Stream:", keys.includes("Stream"));
console.log("class keys:", JSON.stringify(keys.filter((k) => ["Server", "Socket", "Stream"].includes(k))));
console.log("Stream === Socket:", (net as any).Stream === net.Socket);
console.log("Stream name:", (net as any).Stream?.name);
console.log("Stream length:", (net as any).Stream?.length);

const socket = new (net as any).Stream();

console.log("stream instanceof Socket:", socket instanceof net.Socket);
console.log(
  "stream methods:",
  [typeof socket.connect, typeof socket.write, typeof socket.destroy].join(","),
);
socket.destroy();
console.log("destroy called");
