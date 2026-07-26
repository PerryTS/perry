import * as net from "node:net";

for (const args of [[], [{}], [{ host: "127.0.0.1" }]] as any[][]) {
  try {
    const socket = (net.connect as any)(...args);
    socket.on("error", () => {});
    socket.destroy();
    console.log(JSON.stringify(args), "OK");
  } catch (error: any) {
    console.log(JSON.stringify(args), error.name, error.code);
  }
}
