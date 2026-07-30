import * as dgram from "node:dgram";

for (
  const mode of ["default", "port-callback", "port-address", "options"] as const
) {
  const socket = dgram.createSocket("udp4");

  try {
    await new Promise<void>((resolve, reject) => {
      function cleanup() {
        socket.removeListener("listening", onListening);
        socket.removeListener("error", onError);
      }

      function onListening(this: dgram.Socket) {
        cleanup();
        const address = socket.address();
        console.log(
          mode,
          address.address,
          address.family,
          typeof address.port,
          address.port > 0,
          this === socket,
        );
        resolve();
      }

      function onError(error: Error) {
        cleanup();
        reject(error);
      }

      socket.once("error", onError);
      if (mode === "default") {
        socket.once("listening", onListening);
        socket.bind();
      } else if (mode === "port-callback") {
        socket.bind(0, onListening);
      } else if (mode === "port-address") {
        socket.bind(0, "127.0.0.1", onListening);
      } else {
        socket.bind({ port: 0, address: "127.0.0.1" }, onListening);
      }
    });
  } finally {
    const closed = new Promise<void>((resolve) =>
      socket.once("close", resolve)
    );
    socket.close();
    await closed;
  }
}
