import { execFileSync, spawnSync } from "node:child_process";

for (const encoding of ["utf8", "utf-8", "ascii", "latin1"] as const) {
  const result = spawnSync(
    "node",
    ["-e", "process.stdout.write('plain-ascii')"],
    {
      encoding,
    },
  );
  console.log(`spawnSync ${encoding}:`, result.status, result.stdout);
}

for (const encoding of ["utf8", "utf-8", "ascii", "latin1"] as const) {
  const output = execFileSync(
    "node",
    ["-e", "process.stdout.write('file-ascii')"],
    {
      encoding,
    },
  );
  console.log(`execFileSync ${encoding}:`, output);
}
