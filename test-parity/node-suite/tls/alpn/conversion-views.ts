import tls from "node:tls";

const source = Buffer.from([9, 2, 104, 50, 9]);
for (
  const [label, view] of [
    ["buffer", source.subarray(1, 4)],
    ["uint8", new Uint8Array(source.buffer, source.byteOffset + 1, 3)],
    ["dataview", new DataView(source.buffer, source.byteOffset + 1, 3)],
  ] as const
) {
  const out: any = {};
  try {
    tls.convertALPNProtocols(view, out);
    source[2] = 120;
    console.log(
      label + ":",
      Buffer.isBuffer(out.ALPNProtocols),
      out.ALPNProtocols.toString("hex"),
    );
  } catch (err: any) {
    console.log(label + ":", err instanceof TypeError, err.code ?? "none");
  } finally {
    source[2] = 104;
  }
}
