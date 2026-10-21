import { readFileSync } from "node:fs";
import { load } from "./generated/bindings";
declare function gc(): void;

const bytes = readFileSync(process.argv[2]);
const port = Number(process.argv[3]);
function check(ok: boolean, message: string): void { if (!ok) throw new Error(message); }
function instance(network: any) {
  return load(bytes, {}, { memoryBytes: 67108864, fuel: 100000000,
    wasi: { network } });
}
function probe(plugin: any): any { return plugin.exports["perry:network-probe/probe@1.0.0"]; }
const local = { address: "127.0.0.1", port: 0 };
const remote = { address: "127.0.0.1", port };
const wrongPort = port === 65535 ? 65534 : port + 1;
const denied = [
  {},
  { tcp: true, addresses: [local, remote] },
  { udp: true },
  { udp: true, addresses: [{ address: "127.0.0.2" }] },
  { udp: true, addresses: [local] },
  { udp: true, addresses: [local, { address: "127.0.0.1", port: wrongPort }] },
];
for (const network of denied) {
  const plugin = instance(network);
  for (const connected of [true, false]) {
    const result = probe(plugin).exchange(port, connected, "denied");
    check(result.tag === "err" && result.value === "AccessDenied", "UDP denial must come from capability checks");
  }
  plugin.dispose();
}
const allowed = instance({ udp: true, addresses: [local, remote] });
for (let i = 0; i < 3; i++) {
  gc();
  for (const connected of [true, false]) {
    const text = (connected ? "connected:" : "unconnected:") + i;
    const result = probe(allowed).exchange(port, connected, text);
    check(result.tag === "ok" && result.value === "reply:" + text, "UDP round-trip payload");
  }
}
const udpDns = probe(allowed).resolve("localhost");
check(udpDns.tag === "err" && udpDns.value === "PermanentResolverFailure", "UDP grant must not enable DNS");
allowed.dispose();
const dns = instance({ dns: true });
for (let i = 0; i < 3; i++) {
  const result = probe(dns).resolve("localhost");
  check(result.tag === "ok" && result.value.includes("127.0.0.1"), "localhost DNS grant");
  const literal = probe(dns).resolve("127.0.0.1");
  check(literal.tag === "ok" && literal.value.length === 1 && literal.value[0] === "127.0.0.1", "IP resolution");
}
check(probe(dns).resolve("invalid name!").tag === "err", "invalid name must fail");
const dnsUdp = probe(dns).exchange(port, true, "denied");
check(dnsUdp.tag === "err" && dnsUdp.value === "AccessDenied", "DNS must not enable UDP");
dns.dispose();
const fresh = instance({});
const deniedDns = probe(fresh).resolve("localhost");
check(deniedDns.tag === "err" && deniedDns.value === "PermanentResolverFailure", "DNS permission leaked");
const freshUdp = probe(fresh).exchange(port, true, "denied");
check(freshUdp.tag === "err" && freshUdp.value === "AccessDenied", "UDP permission leaked");
fresh.dispose();
console.log("PASS component networking: UDP connected/unconnected traffic, exact grants, DNS, isolation");
