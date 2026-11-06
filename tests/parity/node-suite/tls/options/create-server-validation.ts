import tls from "node:tls";

function probe(label: string, options: any) {
  try {
    const server = tls.createServer(options);
    console.log(label + ":", server instanceof tls.Server);
  } catch (err: any) {
    console.log(label + ":", err instanceof TypeError, err.code);
  }
}
probe("empty", {});
probe("ciphers", { ciphers: 1 });
probe("curve", { ecdhCurve: 1 });
probe("handshake timeout", { handshakeTimeout: "1" });
probe("session timeout", { sessionTimeout: "1" });
probe("ticket keys type", { ticketKeys: "bad" });
probe("ticket keys length", { ticketKeys: Buffer.alloc(0) });
