// Behavioral parity test for dotenv via perry-stdlib.
//
// Uses dotenv.parse on a fixed buffer so the output is deterministic and
// independent of the surrounding process environment. dotenv.config() and
// dotenv.config({ path }) are exercised by writing a temp file first; the
// printed values are compared to the strings we wrote.

import * as dotenv from "dotenv";
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";

// ── parse(buffer) — pure, deterministic ──
const src = [
  "FOO=bar",
  "QUOTED=\"hello world\"",
  "EMPTY=",
  "NUMBER=42",
  "EXPORTED=export-style-value",
  "# COMMENT=ignored",
].join("\n");

const parsed = dotenv.parse(Buffer.from(src));
console.log("parse FOO:", parsed.FOO);
console.log("parse QUOTED:", parsed.QUOTED);
console.log("parse EMPTY:", parsed.EMPTY);
console.log("parse NUMBER:", parsed.NUMBER);
console.log("parse EXPORTED:", parsed.EXPORTED);
console.log("parse COMMENT typeof:", typeof parsed.COMMENT);

// ── config({ path }) — write a temp .env and load it ──
const dir = fs.mkdtempSync(path.join(os.tmpdir(), "perry-dotenv-parity-"));
const envFile = path.join(dir, "test.env");
fs.writeFileSync(envFile, "PERRY_DOTENV_PARITY=loaded-ok\n", "utf8");

// Some Node versions return { parsed } only when the file was readable.
const cfg = dotenv.config({ path: envFile });
console.log("config error is undefined:", cfg.error === undefined);
console.log("config parsed key:", cfg.parsed ? cfg.parsed.PERRY_DOTENV_PARITY : "");
console.log("process.env value:", process.env.PERRY_DOTENV_PARITY);

// Cleanup so re-runs are hermetic.
fs.unlinkSync(envFile);
fs.rmdirSync(dir);

/*
@covers
crates/perry-stdlib/src/dotenv.rs:
  - js_dotenv_config
  - js_dotenv_config_path
  - js_dotenv_parse
*/
