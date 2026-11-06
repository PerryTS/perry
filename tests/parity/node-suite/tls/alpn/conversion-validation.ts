import tls from "node:tls";

function probe(label: string, protocols: any) {
  const out: any = { marker: true };
  try {
    tls.convertALPNProtocols(protocols, out);
    console.log(label + ":", Object.keys(out).sort().join(","));
  } catch (err: any) {
    console.log(
      label + ":",
      err instanceof RangeError || err instanceof TypeError,
      err.code ?? "none",
    );
  }
}
probe("too long", ["a".repeat(256)]);
probe("non string", [1]);
probe("boolean", true);
probe("null", null);
