import { spawnSync } from "node:child_process";

const key = "PERRY_CHILD_SYNC_DEFAULT_ENV";
const previous = process.env[key];
process.env[key] = "sync-inherited";
try {
  const result = spawnSync(
    "node",
    ["-e", `process.stdout.write(process.env.${key} || 'missing')`],
    {
      encoding: "utf8",
    },
  );
  console.log("status:", result.status);
  console.log("stdout:", result.stdout);
  console.log("stderr:", JSON.stringify(result.stderr));
} finally {
  if (previous === undefined) delete process.env[key];
  else process.env[key] = previous;
}
