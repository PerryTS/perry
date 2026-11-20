import tls from "node:tls";

const getAlgorithms = (tls as any).getCertificateCompressionAlgorithms;
console.log("export:", typeof getAlgorithms === "function");
if (typeof getAlgorithms === "function") {
  const first = getAlgorithms();
  const second = getAlgorithms();
  const allowed = new Set(["zlib", "brotli", "zstd"]);
  console.log(
    "shape:",
    Array.isArray(first),
    first.every((value: unknown) =>
      typeof value === "string" && allowed.has(value)
    ),
    first.length === new Set(first).size,
  );
  console.log("fresh:", first !== second, first.join(",") === second.join(","));
}
