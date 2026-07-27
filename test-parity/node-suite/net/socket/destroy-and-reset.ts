import { Socket } from "node:net";

for (const method of ["destroy", "resetAndDestroy"] as const) {
  const socket = new Socket();
  let emittedError: any;
  const closed = new Promise<void>((resolve) => socket.once("close", resolve));
  socket.once("error", (error) => emittedError = error);
  try {
    console.log(method, "return:", socket[method]() === socket);
    console.log(method, "second:", socket[method]() === socket);
    await closed;
  } catch (error: any) {
    console.log(method, error.name, error.code);
  } finally {
    socket.destroy();
  }
  console.log(
    method,
    "error:",
    emittedError?.name ?? "none",
    emittedError?.code ?? "none",
  );
  console.log(method, "state:", socket.destroyed, socket.readyState);
}
