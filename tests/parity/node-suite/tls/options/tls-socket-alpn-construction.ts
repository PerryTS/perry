import tls from "node:tls";

for (
  const [label, protocols] of [
    ["array", ["http/1.1"]],
    ["buffer", Buffer.from([2, 104, 50])],
    ["uint8", new Uint8Array([2, 104, 50])],
  ] as const
) {
  try {
    const socket = new tls.TLSSocket(null as any, {
      ALPNProtocols: protocols as any,
    });
    console.log(label + ":", socket instanceof tls.TLSSocket);
    try {
      socket.destroy();
    } catch {}
  } catch (err: any) {
    console.log(label + ":", false, err.code ?? err.name);
  }
}
for (
  const [label, protocols] of [["long", ["a".repeat(256)]], [
    "boolean",
    true,
  ]] as const
) {
  try {
    const socket = new tls.TLSSocket(null as any, {
      ALPNProtocols: protocols as any,
    });
    console.log(label + ": no throw");
    try {
      socket.destroy();
    } catch {}
  } catch (err: any) {
    console.log(
      label + ":",
      err instanceof TypeError || err instanceof RangeError,
      err.code,
    );
  }
}
