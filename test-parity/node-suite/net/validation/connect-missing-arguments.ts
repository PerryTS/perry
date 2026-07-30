import * as net from "node:net";

async function destroyAndSettle(socket: net.Socket): Promise<void> {
  await new Promise<void>((resolve) => {
    socket.once("error", () => resolve());
    socket.once("close", () => resolve());
    socket.destroy();
  });
}

for (const args of [[], [{}], [{ host: "127.0.0.1" }]] as any[][]) {
  try {
    const socket = (net.connect as any)(...args);
    await destroyAndSettle(socket);
    console.log(JSON.stringify(args), "OK");
  } catch (error: any) {
    console.log(JSON.stringify(args), error.name, error.code);
  }
}
