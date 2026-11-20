import { execFileSync, spawnSync } from "node:child_process";
import { mkdirSync, realpathSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const cwd = join(tmpdir(), `perry-child-process-sync-${process.pid}`);
mkdirSync(cwd);
const expectedCwd = realpathSync(cwd);

try {
  const code = [
    "const fs = require('node:fs');",
    "const input = fs.readFileSync(0);",
    "process.stdout.write(JSON.stringify({",
    "cwd: process.cwd(),",
    "text: input.toString('utf8'),",
    "hex: input.toString('hex'),",
    "value: process.env.PERRY_VALUE,",
    "empty: process.env.PERRY_EMPTY,",
    "missing: Object.hasOwn(process.env, 'PERRY_MISSING')",
    "}));",
  ].join("");
  const env = {
    ...process.env,
    PERRY_VALUE: 42 as any,
    PERRY_EMPTY: "",
    PERRY_MISSING: undefined,
  };

  const spawned = spawnSync("node", ["-e", code], {
    cwd,
    env,
    input: new Uint8Array([65, 0, 66]),
    encoding: "utf8",
  });
  const spawnValue = JSON.parse(spawned.stdout);
  console.log("spawnSync status:", spawned.status);
  console.log("spawnSync cwd:", spawnValue.cwd === expectedCwd);
  console.log("spawnSync input text:", JSON.stringify(spawnValue.text));
  console.log("spawnSync input hex:", spawnValue.hex);
  console.log(
    "spawnSync env:",
    spawnValue.value,
    JSON.stringify(spawnValue.empty),
    spawnValue.missing,
  );
  console.log("spawnSync stderr:", JSON.stringify(spawned.stderr));

  const fileText = execFileSync(
    "node",
    [
      "-e",
      "process.stdout.write(process.cwd() + '|' + process.env.PERRY_VALUE)",
    ],
    {
      cwd,
      env,
      encoding: "utf8",
    },
  );
  console.log("execFileSync cwd/env:", fileText === `${expectedCwd}|42`);
} finally {
  rmSync(cwd, { recursive: true, force: true });
}
