import tls from "node:tls";

function probe(label: string, options: any) {
  try {
    const socket = tls.connect(options);
    socket.on("error", () => {});
    socket.destroy();
    console.log(label + ": no throw");
  } catch (err: any) {
    console.log(
      label + ":",
      err instanceof TypeError || err instanceof RangeError,
      err.code,
    );
  }
}
probe("missing port", {});
probe("port type", { port: "bad" });
probe("port range", { port: 70000 });
probe("identity null", { port: 1, checkServerIdentity: null });
probe("identity number", { port: 1, checkServerIdentity: 1 });
probe("identity undefined", { port: 1, checkServerIdentity: undefined });
probe("identity boolean", { port: 1, checkServerIdentity: true });
probe("alpn type", { port: 1, ALPNProtocols: true });
