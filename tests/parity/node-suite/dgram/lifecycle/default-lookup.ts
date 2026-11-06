// Upstream: Node v26.5.0 test/parallel/test-dgram-default-lookup-ip.js.
// Coverage added: default literal and mismatched-family lookup dispatch.
import * as dgram from "node:dgram";
import dns from "node:dns";

type Lookup = typeof dns.lookup;
const originalLookup = dns.lookup;
const calls: string[] = [];
const sockets: dgram.Socket[] = [];

try {
  dns.lookup = ((
    hostname: string,
    family: number,
    callback: (error: null, address: string, family: number) => void,
  ) => {
    calls.push(`${hostname}:${family}`);
    callback(null, "127.0.0.1", 4);
  }) as Lookup;

  const literal = dgram.createSocket("udp4");
  sockets.push(literal);
  await new Promise<void>((resolve) => literal.bind(0, "127.0.0.1", resolve));
  console.log("literal lookup calls:", calls.length);

  const mismatched = dgram.createSocket("udp4");
  sockets.push(mismatched);
  const result = await new Promise<string>((resolve) => {
    mismatched.once("error", (error) => resolve(error.code));
    mismatched.bind(0, "::1", () => resolve("listening"));
  });
  console.log("mismatched lookup:", result, calls.join(","));
} finally {
  dns.lookup = originalLookup;
  await Promise.all(sockets.map((socket) => {
    const closed = new Promise<void>((resolve) =>
      socket.once("close", resolve)
    );
    try {
      socket.close();
      return closed;
    } catch (error: unknown) {
      if (
        (error as { code?: string }).code === "ERR_SOCKET_DGRAM_NOT_RUNNING"
      ) {
        return Promise.resolve();
      }
      return Promise.reject(error);
    }
  }));
}
