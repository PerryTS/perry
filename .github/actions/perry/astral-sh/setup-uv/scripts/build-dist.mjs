import { rm } from "node:fs/promises";
import { build } from "esbuild";

const builds = [
  {
    entryPoints: ["src/setup-uv.ts"],
    outfile: "dist/setup/index.cjs",
    staleOutfiles: ["dist/setup/index.mjs"],
  },
  {
    entryPoints: ["src/save-cache.ts"],
    outfile: "dist/save-cache/index.cjs",
    staleOutfiles: ["dist/save-cache/index.mjs"],
  },
  {
    entryPoints: ["src/update-known-checksums.ts"],
    outfile: "dist/update-known-checksums/index.cjs",
    staleOutfiles: ["dist/update-known-checksums/index.mjs"],
  },
];

for (const { staleOutfiles, ...options } of builds) {
  await Promise.all(
    staleOutfiles.map((outfile) => rm(outfile, { force: true })),
  );
  await build({
    bundle: true,
    format: "cjs",
    platform: "node",
    target: "node24",
    ...options,
  });
}
