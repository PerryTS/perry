import { WASI } from "node:wasi";

const W: any = WASI;
for (const version of ["preview1", "unstable"] as const) {
  const wasi = new W({ version });
  const namespace = version === "preview1"
    ? "wasi_snapshot_preview1"
    : "wasi_unstable";
  const original = wasi.wasiImport;
  const replacement = { version };

  try {
    wasi.wasiImport = replacement;
    console.log(version, "assignment: ok");
  } catch (error: any) {
    console.log(
      version,
      "assignment: throw",
      error?.name,
      error?.code || "no-code",
    );
  }
  console.log(version, "instance replaced:", wasi.wasiImport === replacement);
  console.log(
    version,
    "instance retained original:",
    wasi.wasiImport === original,
  );

  try {
    const wrapper = wasi.getImportObject();
    console.log(version, "wrapper keys:", Object.keys(wrapper).join(","));
    console.log(
      version,
      "wrapper reflects replacement:",
      wrapper[namespace] === replacement,
    );
  } catch (error: any) {
    console.log(
      version,
      "wrapper: throw",
      error?.name,
      error?.code || "no-code",
    );
  }
}
