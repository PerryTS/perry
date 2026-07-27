import * as dgram from "node:dgram";

for (const mode of ["port-address", "options"] as const) {
  const socket = dgram.createSocket("udp4");

  try {
    await new Promise<void>((resolve) => {
      function onListening(this: dgram.Socket) {
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

      if (mode === "port-address") {
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
