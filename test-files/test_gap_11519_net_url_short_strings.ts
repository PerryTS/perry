// #11519: node:net event names and BlockList addresses, URL / URLSearchParams
// strings and AbortSignal event names given as SHORT (SSO, <= 5 bytes, built
// at runtime) strings. The net natives masked the event name into an address;
// the URL helpers read the heap string tag only, so the value read as "".
import * as net from "node:net";
import * as url from "node:url";
const S = (s: string): string => s.charAt(0) + s.slice(1);

const p = new URLSearchParams(S("a=1"));
console.log("URLSearchParams:", p.get(S("a")), p.has(S("a")), p.toString());
p.forEach((v, k) => console.log("forEach:", k, v));
console.log("url.parse:", url.parse(S("a/b")).pathname, url.parse(S("?q=1")).query);
console.log("URL:", new URL(S("/x"), "http://h.test/").href);

const ac = new AbortController();
ac.signal.addEventListener(S("abort"), () => console.log("abort listener fired"));
ac.abort();

const bl = new net.BlockList();
bl.addAddress(S("::1"), S("ipv6"));
console.log("BlockList:", bl.check("::1", "ipv6"), bl.check(S("::2"), S("ipv6")));

const server = net.createServer((sock) => {
  let got = "";
  sock.on(S("data"), (d: any) => (got += d));
  sock.on(S("end"), () => {
    sock.end(S("pong"));
    console.log("server got:", got);
  });
});
server.listen(0, "127.0.0.1", () => {
  const port = (server.address() as any).port;
  const c = net.connect(port, "127.0.0.1", () => c.end(S("ping")));
  let reply = "";
  c.on(S("data"), (d: any) => (reply += d));
  c.on(S("close"), () => {
    console.log("client got:", reply, "listeners:", c.listenerCount(S("data")));
    server.close();
  });
});
