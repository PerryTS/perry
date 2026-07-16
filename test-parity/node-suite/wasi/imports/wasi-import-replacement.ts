import { WASI } from "node:wasi";

const W: any = WASI;
for (const version of ["preview1", "unstable"] as const) {
  const wasi = new W({ version });
  const namespace = version === "preview1"
    ? "wasi_snapshot_preview1"
    : "wasi_unstable";
  const original = wasi.wasiImport;
  const replacement = { version };

  wasi.wasiImport = replacement;
  const wrapper = wasi.getImportObject();
  console.log(version, "instance replaced:", wasi.wasiImport === replacement);
  console.log(version, "wrapper keys:", Object.keys(wrapper).join(","));
  console.log(
    version,
    "wrapper reflects replacement:",
    wrapper[namespace] === replacement,
  );
  console.log(version, "original unchanged:", original !== replacement);
}
