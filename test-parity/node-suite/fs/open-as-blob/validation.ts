import * as fs from "node:fs";

const ROOT = "/tmp/perry_node_suite_fs_open_as_blob_validation";
try {
  fs.rmSync(ROOT, { recursive: true, force: true });
} catch (_e) {}
fs.mkdirSync(ROOT, { recursive: true });

const file = ROOT + "/file.txt";
fs.writeFileSync(file, "valid");

function reportThrow(label: string, fn: () => unknown) {
  try {
    const value = fn();
    console.log(label, "ok", value instanceof Promise);
  } catch (e: any) {
    console.log(label, "throw", e.name, e.code, e.message);
  }
}

reportThrow("bad path", () => fs.openAsBlob(1 as never));
reportThrow("missing path arg", () => fs.openAsBlob());
reportThrow("bad options before bad path", () => fs.openAsBlob(1 as never, null as never));
reportThrow("bad type before bad path", () => fs.openAsBlob(1 as never, { type: true } as never));
reportThrow("bad options null", () => fs.openAsBlob(file, null as never));
reportThrow("bad options array", () => fs.openAsBlob(file, [] as never));
reportThrow("bad type true", () => fs.openAsBlob(file, { type: true } as never));
reportThrow("bad type number", () => fs.openAsBlob(file, { type: 1 } as never));
reportThrow("bad type object", () => fs.openAsBlob(file, { type: {} } as never));
reportThrow("missing", () => fs.openAsBlob(ROOT + "/missing.txt"));

for (const type of [undefined, null, false, 0, NaN, ""]) {
  const blob = await fs.openAsBlob(file, { type } as never);
  console.log("falsy type:", String(type), JSON.stringify(blob.type));
}

const typed = await fs.openAsBlob(file, { type: "Text/Plain" });
console.log("type casing:", typed.type);
console.log("promises openAsBlob:", "openAsBlob" in fs.promises, typeof (fs.promises as any).openAsBlob);
