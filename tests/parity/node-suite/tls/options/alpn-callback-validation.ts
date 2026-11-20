import tls from "node:tls";

try {
  tls.createServer({ ALPNCallback: () => "h2", ALPNProtocols: ["h2"] });
  console.log("conflict: no throw");
} catch (err: any) {
  console.log("conflict:", err instanceof TypeError, err.code);
}
console.log("callback only:", tls.createServer({ ALPNCallback: () => "h2" }) instanceof tls.Server);
console.log("null callback:", tls.createServer({ ALPNCallback: null as any, ALPNProtocols: ["h2"] }) instanceof tls.Server);
