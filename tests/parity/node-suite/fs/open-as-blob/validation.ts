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
reportThrow("bad falsy type before bad path", () => fs.openAsBlob(1 as never, { type: 0 } as never));
reportThrow("bad type before missing path", () => fs.openAsBlob(ROOT + "/missing.txt", { type: false } as never));
reportThrow("bad options null", () => fs.openAsBlob(file, null as never));
reportThrow("bad options array", () => fs.openAsBlob(file, [] as never));
reportThrow("bad type true", () => fs.openAsBlob(file, { type: true } as never));
reportThrow("bad type number", () => fs.openAsBlob(file, { type: 1 } as never));
reportThrow("bad type object", () => fs.openAsBlob(file, { type: {} } as never));
reportThrow("missing", () => fs.openAsBlob(ROOT + "/missing.txt"));

async function reportType(type: unknown) {
  try {
    const blob = await fs.openAsBlob(file, { type } as never);
    console.log("falsy type:", String(type), JSON.stringify(blob.type));
    console.log("falsy content:", String(type), await blob.text());
  } catch (err: any) {
    // Only the explicitly invalid MIME-type cases may report validation errors.
    if (type === undefined || typeof type === "string" || err.code !== "ERR_INVALID_ARG_TYPE") {
      throw err;
    }
    console.log("falsy type:", String(type), "throw", err.name, err.code, err.message);
  }
}

for (const type of [undefined, null, false, 0, NaN, ""]) {
  await reportType(type);
}

const typed = await fs.openAsBlob(file, { type: "Text/Plain" });
console.log("type casing:", typed.type);
console.log("typed content:", await typed.text());
console.log("promises openAsBlob:", "openAsBlob" in fs.promises, typeof (fs.promises as any).openAsBlob);
