import * as dgram from "node:dgram";

const group = "224.0.0.114";
const iface = "127.0.0.1";
const socket = dgram.createSocket({ type: "udp4", reuseAddr: true });

await new Promise<void>((resolve, reject) => {
  const timeout = setTimeout(() => reject(new Error("multicast timeout")), 3000);
  socket.once("error", reject);
  socket.once("message", (message, rinfo) => {
    clearTimeout(timeout);
    console.log(
      "multicast message:",
      message.toString(),
      rinfo.family,
      rinfo.size,
    );
    resolve();
  });
  socket.bind(0, "0.0.0.0", () => {
    socket.addMembership(group, iface);
    socket.setMulticastInterface(iface);
    socket.setMulticastTTL(16);
    socket.setMulticastLoopback(true);
    socket.send("bonjour", socket.address().port, group);
  });
});

socket.dropMembership(group, iface);
await new Promise<void>((resolve) => socket.close(resolve));
console.log("membership cleanup: true");
