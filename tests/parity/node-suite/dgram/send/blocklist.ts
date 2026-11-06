import * as dgram from "node:dgram";
import { BlockList } from "node:net";

const blockList = new BlockList();
blockList.addAddress("127.0.0.1");

const target = dgram.createSocket("udp4");
const sendSocket = dgram.createSocket({
  type: "udp4",
  sendBlockList: blockList,
});
const connectSocket = dgram.createSocket({
  type: "udp4",
  sendBlockList: blockList,
});
try {
  await Promise.all([
    new Promise<void>((resolve) => target.bind(0, "127.0.0.1", resolve)),
    new Promise<void>((resolve) => sendSocket.bind(0, "127.0.0.1", resolve)),
    new Promise<void>((resolve) => connectSocket.bind(0, "127.0.0.1", resolve)),
  ]);
  const port = target.address().port;

  const sendCode = await new Promise<string>((resolve) => {
    sendSocket.send("blocked", port, "127.0.0.1", (error) => {
      resolve(error?.code ?? "none");
    });
  });
  console.log("blocked send:", sendCode);

  const connectCode = await new Promise<string>((resolve) => {
    connectSocket.connect(port, "127.0.0.1", (error) => {
      resolve(error?.code ?? "none");
    });
  });
  console.log("blocked connect:", connectCode);
} finally {
  await Promise.all([
    new Promise<void>((resolve) => sendSocket.close(resolve)),
    new Promise<void>((resolve) => connectSocket.close(resolve)),
    new Promise<void>((resolve) => target.close(resolve)),
  ]);
}
