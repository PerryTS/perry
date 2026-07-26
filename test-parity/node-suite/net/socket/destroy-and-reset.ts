import { Socket } from "node:net";

for (const method of ["destroy", "resetAndDestroy"] as const) {
  const socket = new Socket();
  socket.on("error", () => {});
  try {
    console.log(method, "return:", socket[method]() === socket);
    console.log(method, "second:", socket[method]() === socket);
  } catch (error: any) {
    console.log(method, error.name, error.code);
  } finally {
    socket.destroy();
  }
  console.log(method, "state:", socket.destroyed, socket.readyState);
}
