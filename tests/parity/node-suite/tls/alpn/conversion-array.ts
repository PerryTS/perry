import tls from "node:tls";

try {
  const out: any = {};
  console.log(
    "return:",
    tls.convertALPNProtocols(["h2", "http/1.1"], out) === undefined,
  );
  console.log("buffer:", Buffer.isBuffer(out.ALPNProtocols));
  console.log("encoding:", out.ALPNProtocols.toString("hex"));

  const empty: any = {};
  tls.convertALPNProtocols([], empty);
  console.log(
    "empty:",
    Buffer.isBuffer(empty.ALPNProtocols),
    empty.ALPNProtocols.length,
  );
} catch (err: any) {
  console.log("error:", err instanceof TypeError, err.code ?? "none");
}
