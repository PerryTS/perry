import * as childProcess from "node:child_process";
import * as fs from "node:fs";
import { randomBytes } from "node:crypto";
import { tmpdir } from "node:os";
import { join } from "node:path";

const marker = "__perry_9487_exit_child__";
const markerIndex = process.argv.indexOf(marker);

// Minified bundles reuse short names aggressively. An earlier assignment from
// a native module used to leave module-wide receiver metadata behind for `z`,
// even though this branch is unreachable and the later destructured `z` is an
// unrelated lexical binding.
let z: unknown;
if (false) z = childProcess.spawnSync(process.execPath, ["--version"]);

function fillNursery(): object[] {
  const retained: object[] = [];
  for (let index = 0; index < 80_000; index++) {
    retained.push({ index, label: "exit-" + index });
  }
  return retained;
}

async function runInstallCompletion(
  message: string,
  configPath: string,
): Promise<void> {
  const command = await Promise.resolve({
    install: {
      async call(
        onDone: (result: string, metadata: object) => void,
        _options: object,
        _args: string[],
      ): Promise<void> {
        setTimeout(onDone, 0, message, { display: "system" });
      },
    },
  });
  const { install: z } = command;

  await new Promise<void>((resolve) => {
    z.call(
      (result) => {
        const retained = fillNursery();
        const userID = randomBytes(32).toString("hex");
        fs.writeFileSync(configPath, JSON.stringify({ migrationVersion: 11, userID }));
        // Match Claude Code 2.1.112's ordering: resolve the outer promise, then
        // synchronously terminate from the same callback.
        resolve();
        if (retained.length !== 80_000) process.exit(3);
        process.exit(result.includes("failed") ? 1 : 0);
      },
      {},
      [],
    );
  });
}

if (markerIndex >= 0) {
  const outcome = process.argv[markerIndex + 1];
  const configPath = process.argv[markerIndex + 2];
  const message = outcome === "failure"
    ? "Claude Code installation failed"
    : "Claude Code installation completed successfully";
  await runInstallCompletion(message, configPath);
} else {
  const script = process.argv[1];
  const selfArgs = (outcome: string, configPath: string): string[] =>
    typeof script === "string" && script.endsWith(".ts")
      ? [script, marker, outcome, configPath]
      : [marker, outcome, configPath];

  for (const outcome of ["failure", "success"]) {
    const configPath = join(tmpdir(), `perry-9487-${process.pid}-${outcome}.json`);
    try {
      try {
        fs.unlinkSync(configPath);
      } catch {}
      const result = childProcess.spawnSync(
        process.execPath,
        selfArgs(outcome, configPath),
        { encoding: "utf8" },
      );
      let hasUserID = false;
      if (fs.existsSync(configPath)) {
        const config = JSON.parse(fs.readFileSync(configPath, "utf8"));
        hasUserID = typeof config.userID === "string" &&
          /^[0-9a-f]{64}$/.test(config.userID);
      }
      console.log(outcome, "status", result.status, "userID", hasUserID);
    } finally {
      try {
        fs.unlinkSync(configPath);
      } catch {}
    }
  }
}
