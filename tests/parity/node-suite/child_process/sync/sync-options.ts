import { execFileSync, execSync, spawnSync } from "node:child_process";

function text(value) {
  return value === null ? "null" : String(value);
}

function reportThrow(label, run) {
  try {
    console.log(`${label} no throw:`, text(run()));
  } catch (err) {
    console.log(`${label} caught:`, err instanceof Error);
    console.log(`${label} code:`, err.code);
    console.log(`${label} status:`, err.status);
    console.log(`${label} signal:`, err.signal);
    console.log(`${label} stdout:`, text(err.stdout));
    console.log(`${label} stderr:`, text(err.stderr));
  }
}

const spawnInput = spawnSync("cat", [], { input: "spawn-in", encoding: "utf8" });
console.log("spawn input stdout:", spawnInput.stdout);
console.log("spawn input stderr:", spawnInput.stderr);
console.log("spawn input status:", spawnInput.status);
console.log("spawn input error:", spawnInput.error && spawnInput.error.code);

const spawnNullInput = spawnSync("cat", [], { input: null, encoding: "utf8" });
console.log("spawn null input stdout:", spawnNullInput.stdout);
console.log("spawn null input status:", spawnNullInput.status);

const spawnBufferInput = spawnSync("cat", [], { input: Buffer.from("buf-in"), encoding: "utf8" });
console.log("spawn buffer input stdout:", spawnBufferInput.stdout);

console.log(
  "execFile input:",
  execFileSync("cat", [], { input: "file-in", encoding: "utf8" }),
);
console.log("exec input:", execSync("cat", { input: "exec-in", encoding: "utf8" }));
console.log("execFile null input:", execFileSync("cat", [], { input: null, encoding: "utf8" }));
console.log("exec null input:", execSync("cat", { input: null, encoding: "utf8" }));

reportThrow("spawn input number", () =>
  spawnSync("cat", [], { input: 123, encoding: "utf8" })
);
reportThrow("execFile input number", () =>
  execFileSync("cat", [], { input: 123, encoding: "utf8" })
);
reportThrow("exec input number", () =>
  execSync("cat", { input: 123, encoding: "utf8" })
);

const spawnMax = spawnSync("sh", ["-c", "printf 123456"], {
  maxBuffer: 3,
  encoding: "utf8",
});
console.log("spawn max stdout:", spawnMax.stdout);
console.log("spawn max status:", spawnMax.status);
console.log("spawn max signal:", spawnMax.signal);
console.log("spawn max error code:", spawnMax.error && spawnMax.error.code);
console.log("spawn max error syscall:", spawnMax.error && spawnMax.error.syscall);

reportThrow("execFile max", () =>
  execFileSync("sh", ["-c", "printf 123456"], { maxBuffer: 3, encoding: "utf8" })
);
reportThrow("exec max", () =>
  execSync("printf 123456", { maxBuffer: 3, encoding: "utf8" })
);

const spawnTimeout = spawnSync("sh", ["-c", "sleep 1"], {
  timeout: 50,
  encoding: "utf8",
});
console.log("spawn timeout status:", spawnTimeout.status);
console.log("spawn timeout signal:", spawnTimeout.signal);
console.log("spawn timeout error code:", spawnTimeout.error && spawnTimeout.error.code);
console.log("spawn timeout error syscall:", spawnTimeout.error && spawnTimeout.error.syscall);

reportThrow("execFile timeout", () =>
  execFileSync("sh", ["-c", "sleep 1"], { timeout: 50, encoding: "utf8" })
);
reportThrow("exec timeout", () =>
  execSync("sleep 1", { timeout: 50, encoding: "utf8" })
);
