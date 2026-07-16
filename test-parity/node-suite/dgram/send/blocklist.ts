import * as dgram from "node:dgram";
import { BlockList } from "node:net";

const blockList = new BlockList();
blockList.addAddress("127.0.0.1");

const sendSocket = dgram.createSocket({ type: "udp4", sendBlockList: blockList });
const sendCode = await new Promise<string>((resolve) => {
  sendSocket.send("blocked", 12345, "127.0.0.1", (error) => {
    resolve(error?.code ?? "none");
  });
});
console.log("blocked send:", sendCode);
await new Promise<void>((resolve) => sendSocket.close(() => resolve()));

const connectSocket = dgram.createSocket({ type: "udp4", sendBlockList: blockList });
const connectCode = await new Promise<string>((resolve) => {
  connectSocket.connect(12345, "127.0.0.1", (error) => {
    resolve(error?.code ?? "none");
  });
});
console.log("blocked connect:", connectCode);
await new Promise<void>((resolve) => connectSocket.close(() => resolve()));
