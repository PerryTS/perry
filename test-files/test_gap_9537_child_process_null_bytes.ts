// #9537 — every child_process spelling must reject embedded null bytes
// synchronously, before a command reaches the OS. The direct-import lowering
// already called setup validators, but CJS-default namespace dispatch skipped
// them and surfaced an asynchronous `UNKNOWN` spawn error instead.
//
// Byte-for-byte vs `node --experimental-strip-types`.
import * as direct from "node:child_process";
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
const cjs: any = require("child_process");

function probe(label: string, run: () => any): void {
  try {
    const value = run();
    // Keep the broken async-spawn path from turning the fixture's intended
    // mismatch into an unhandled `error` event on a pre-fix binary.
    if (value && typeof value.on === "function") value.on("error", () => {});
    console.log(label, "NO_THROW");
  } catch (e: any) {
    console.log(label, "|", e.name, "|", e.code, "|", e.message);
  }
}

// Existing direct-import path: pin the corrected Node message, including the
// argument name, original array index, escaped received value, class and code.
probe("direct spawn file", () => direct.spawn("/bin/tr\x00ue", []));
probe("direct spawn args[1]", () => direct.spawn("/bin/true", ["ok", "a\x00b"]));
probe("direct spawn escaped received", () =>
  direct.spawn("/bin/true", ["a'\\\n\x00b"]));
probe("direct exec command", () => direct.exec("echo\x00x", () => {}));
probe("direct execFileSync args[1]", () =>
  direct.execFileSync("/bin/true", ["ok", "a\x00b"]));

// Regression path: fused method calls on require("child_process") route
// through the native-module dispatcher and then the runtime entry points.
probe("cjs spawn file", () => cjs.spawn("/bin/tr\x00ue", []));
probe("cjs spawn args[1]", () => cjs.spawn("/bin/true", ["ok", "a\x00b"]));
probe("cjs spawnSync args[1]", () => cjs.spawnSync("/bin/true", ["ok", "a\x00b"]));
probe("cjs exec command", () => cjs.exec("echo\x00x", () => {}));
probe("cjs execSync command", () => cjs.execSync("echo\x00x"));
probe("cjs execFile args[1]", () =>
  cjs.execFile("/bin/true", ["ok", "a\x00b"], () => {}));
probe("cjs execFileSync file", () => cjs.execFileSync("/bin/tr\x00ue", []));

// OS-facing option strings use Node's `property` wording. `cwd` is PathLike,
// so its expected-type clause additionally names Uint8Array and URL.
probe("cjs cwd", () => cjs.spawn("/bin/true", [], { cwd: "/tmp/a\x00b" }));
probe("cjs argv0", () => cjs.spawn("/bin/true", [], { argv0: "a\x00b" }));
probe("cjs shell", () => cjs.spawn("true", [], { shell: "/bin/s\x00h" }));
probe("cjs env value", () => cjs.spawn("/bin/true", [], { env: { A: "a\x00b" } }));
probe("cjs env undefined key", () =>
  cjs.spawn("/bin/true", [], { env: { ["A\x00B"]: undefined } }));

console.log("done");
