import tls from "node:tls";
import net from "node:net";

const socket = new tls.TLSSocket(new net.Socket());
for (const value of [511, 512, 16384, 16385, "1024"] as any[]) {
  try {
    console.log(String(value) + ":", socket.setMaxSendFragment(value));
  } catch (err: any) {
    console.log(String(value) + ":", err instanceof TypeError || err instanceof RangeError, err.code);
  }
}
socket.destroy();
