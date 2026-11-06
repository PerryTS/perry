// A real local fetch body and connected socket stay alive through every check.
// Subnormal doubles also exercise the former raw-id instanceof decoder.
import { createServer } from "node:http";
import { createServer as createNetServer, createConnection, Socket } from "node:net";

function inspect(n: any) {
  const obj: any = {};
  obj[n] = "value";
  const map = new Map<any, string>();
  map.set(n, "number");
  const set = new Set<any>();
  set.add(n);
  console.log("number", n, typeof n, n === Number(n), n === 1048576, n < 0x100000, n < 0x200001,
    Number.isInteger(n), obj[String(n)], map.get(Number(n)), set.has(Number(n)),
    JSON.parse(JSON.stringify(n)) === n, n instanceof Socket,
    n instanceof Response, n instanceof ReadableStream,
    Object.prototype.toString.call(n));
}

function typedExpando(stream: ReadableStream<Uint8Array>) {
  (stream as any).typedMarker = "typed";
  console.log("typed-expando", (stream as any).typedMarker);
}

async function main() {
  const peers: any[] = [];
  const tcp = createNetServer(peer => { peers.push(peer); });
  await new Promise<void>(resolve => tcp.listen(0, "127.0.0.1", resolve));
  const socket = createConnection({ host: "127.0.0.1", port: (tcp.address() as any).port });
  await new Promise<void>(resolve => socket.once("connect", resolve));
  const http = createServer((_req, res) => { res.end("local-body"); });
  await new Promise<void>(resolve => http.listen(0, "127.0.0.1", resolve));
  const response = await fetch("http://127.0.0.1:" + (http.address() as any).port);
  const body = response.body!;
  console.log("live", socket instanceof Socket, response instanceof Response,
    body instanceof ReadableStream, typeof body, body.locked);
  typedExpando(body);
  const samples = JSON.parse("[1,1048575,1048576,1048577,1048580,1572864,2097150,2097151,2097152]");
  for (const n of samples) inspect(n);
  const keyed: any = {};
  keyed[1048576] = "numeric";
  console.log("key", keyed["1048576"], Object.keys(keyed).join(","));
  const map = new Map<any, string>([[1048576, "number"], [body, "stream"], [socket, "socket"]]);
  const set = new Set<any>([1048576, body, socket, 1048576]);
  console.log("distinct", map.size, map.get(1048576), map.get(body), map.get(socket), set.size);

  const bits = new DataView(new ArrayBuffer(8));
  let falseResponseBrands = 0;
  // Fetch registry ids begin at 0x40000. These doubles are JS numbers,
  // even when their IEEE payload happens to equal a live native id.
  for (let id = 0x40000; id < 0x40020; id++) {
    bits.setUint32(0, id, true);
    bits.setUint32(4, 0, true);
    const n: any = bits.getFloat64(0, true);
    if (n instanceof Response) falseResponseBrands++;
  }
  console.log("subnormal-response-brands", falseResponseBrands);
  console.log("subnormal-socket", (Number.MIN_VALUE as any) instanceof Socket);
  console.log("body", await response.text());
  socket.destroy();
  for (const peer of peers) peer.destroy();
  await new Promise<void>(resolve => tcp.close(() => resolve()));
  await new Promise<void>(resolve => http.close(() => resolve()));
}
main().catch(error => { console.log("error", error.name, error.message); process.exitCode = 1; });

