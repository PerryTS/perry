import { WASI } from "node:wasi";

const W: any = WASI;

for (const version of ["preview1", "unstable"] as const) {
  const wasi = new W({ version });
  const namespace = version === "preview1"
    ? "wasi_snapshot_preview1"
    : "wasi_unstable";
  if (typeof wasi.getImportObject !== "function") {
    console.log(version, "getImportObject: unavailable");
    continue;
  }
  const first: any = wasi.getImportObject();
  const second: any = wasi.getImportObject();

  console.log(version, "keys:", Object.keys(first).join(","));
  console.log(
    version,
    "import identity:",
    first[namespace] === wasi.wasiImport,
  );
  console.log(version, "fresh wrapper:", first !== second);
  console.log(
    version,
    "stable import:",
    first[namespace] === second[namespace],
  );
}
