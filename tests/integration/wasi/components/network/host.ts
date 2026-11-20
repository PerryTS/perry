import { readFileSync } from "node:fs";
import { load } from "./generated/bindings";
declare function gc(): void;

const bytes = readFileSync(process.argv[2]);
const port = Number(process.argv[3]);
const tcpPort = Number(process.argv[4]);
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
    check(result.tag === "err" && result.value === "access-denied", "UDP denial must come from capability checks: " + JSON.stringify(result) + " network=" + JSON.stringify(network) + " connected=" + connected);
  }
  plugin.dispose();
}
const allowed = instance({ udp: true, addresses: [local, remote] });
for (let i = 0; i < 3; i++) {
  gc();
  for (const connected of [true, false]) {
    const text = (connected ? "connected:" : "unconnected:") + i;
    const result = probe(allowed).exchange(port, connected, text);
    check(result.tag === "ok" && result.value === "reply:" + text, "UDP round-trip payload: " + JSON.stringify(result));
  }
}
const udpDns = probe(allowed).resolve("localhost");
check(udpDns.tag === "err" && udpDns.value === "permanent-resolver-failure", "UDP grant must not enable DNS");
allowed.dispose();
const tcpRemote = { address: "127.0.0.1", port: tcpPort };
const wrongTcpPort = tcpPort === 65535 ? 65534 : tcpPort + 1;
for (const network of [
  {},
  { udp: true, addresses: [local, tcpRemote] },
  { tcp: true },
  { tcp: true, addresses: [tcpRemote] },
  { tcp: true, addresses: [local] },
  { tcp: true, addresses: [local, { address: "127.0.0.2", port: tcpPort }] },
  { tcp: true, addresses: [local, { address: "127.0.0.1", port: wrongTcpPort }] },
]) {
  const plugin = instance(network);
  const result = probe(plugin).connect(tcpPort, "denied");
  check(result.tag === "err" && result.value === "access-denied", "TCP denial must come from capability checks: " + JSON.stringify(result) + " network=" + JSON.stringify(network));
  plugin.dispose();
}
const tcp = instance({ tcp: true, addresses: [local, tcpRemote] });
for (let i = 0; i < 3; i++) {
  gc();
  const text = "tcp:" + i;
  const result = probe(tcp).connect(tcpPort, text);
  check(result.tag === "ok" && result.value === "reply:" + text, "TCP round-trip payload: " + JSON.stringify(result) + " port=" + tcpPort + " payload=" + text);
}
const tcpDns = probe(tcp).resolve("localhost");
check(tcpDns.tag === "err" && tcpDns.value === "permanent-resolver-failure", "TCP grant must not enable DNS");
tcp.dispose();
const dns = instance({ dns: true });
for (let i = 0; i < 3; i++) {
  const result = probe(dns).resolve("localhost");
  check(result.tag === "ok" && result.value.includes("127.0.0.1"), "localhost DNS grant");
  const literal = probe(dns).resolve("127.0.0.1");
  check(literal.tag === "ok" && literal.value.length === 1 && literal.value[0] === "127.0.0.1", "IP resolution");
}
check(probe(dns).resolve("invalid name!").tag === "err", "invalid name must fail");
const dnsUdp = probe(dns).exchange(port, true, "denied");
check(dnsUdp.tag === "err" && dnsUdp.value === "access-denied", "DNS must not enable UDP");
const dnsTcp = probe(dns).connect(tcpPort, "denied");
check(dnsTcp.tag === "err" && dnsTcp.value === "access-denied", "DNS must not enable TCP");
dns.dispose();
const fresh = instance({});
const deniedDns = probe(fresh).resolve("localhost");
check(deniedDns.tag === "err" && deniedDns.value === "permanent-resolver-failure", "DNS permission leaked");
const freshUdp = probe(fresh).exchange(port, true, "denied");
check(freshUdp.tag === "err" && freshUdp.value === "access-denied", "UDP permission leaked");
const freshTcp = probe(fresh).connect(tcpPort, "denied");
check(freshTcp.tag === "err" && freshTcp.value === "access-denied", "TCP permission leaked");
fresh.dispose();
// A JS timer becomes due while the guest is blocked in a WASI clock poll.
// The guest may turn the shared native driver, but JS must not re-enter here.
const waiting = instance({});
let inGuest = true;
let callbacks = 0;
setTimeout(() => {
  check(!inGuest, "JS timer re-entered a synchronous guest call");
  callbacks++;
}, 1);
probe(waiting).wait(50);
inGuest = false;
check(callbacks === 0, "guest wait dispatched JS before returning");
waiting.dispose();
setTimeout(() => {
  check(callbacks === 1, "due JS callback must run exactly once after guest return");
  console.log("PASS component networking: TCP/UDP traffic, exact grants, DNS, isolation");
}, 5);
