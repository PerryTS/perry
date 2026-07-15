import * as dgram from "node:dgram";

for (const mode of ["default", "port", "options"] as const) {
  const socket = dgram.createSocket("udp4");

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
      socket.close(() => resolve());
    }

    if (mode === "default") {
      socket.bind(onListening);
    } else if (mode === "port") {
      socket.bind(0, onListening);
    } else {
      socket.bind({ port: 0, address: "127.0.0.1" }, onListening);
    }
  });
}
