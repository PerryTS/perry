import tls from "node:tls";

function probe(label: string, protocols: any) {
  try {
    const socket = tls.connect({ port: 1, ALPNProtocols: protocols });
    socket.on("error", () => {});
    socket.destroy();
    console.log(label + ": ok");
  } catch (err: any) {
    console.log(label + ":", err instanceof TypeError || err instanceof RangeError, err.code);
  }
}
probe("array", ["h2", "http/1.1"]);
probe("buffer", Buffer.from([2, 104, 50]));
probe("long name", ["a".repeat(256)]);
probe("non string", [1]);
probe("boolean", true);
