import tls from "node:tls";

for (const servername of ["127.0.0.1", "::1"]) {
  try {
    const socket = tls.connect({ port: 1, servername });
    socket.on("error", () => {});
    socket.destroy();
    console.log(servername + ": no throw");
  } catch (err: any) {
    console.log(servername + ":", err instanceof TypeError, err.code);
  }
}
