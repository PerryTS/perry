import { execFileSync } from "node:child_process";
import { createReadStream, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const chunkSize = 50_000;
const direct = execFileSync(
  "node",
  [
    "-e",
    `process.stdout.write('A'.repeat(${chunkSize})); process.stdout.write('B'.repeat(${chunkSize}));`,
  ],
  { encoding: "utf8", maxBuffer: 1024 * 1024 },
);
console.log("direct length:", direct.length);
console.log(
  "direct boundary:",
  direct[0] + direct[chunkSize - 1] + direct[chunkSize] + direct.at(-1),
);

const dataFile = join(
  tmpdir(),
  `perry-child-process-large-output-${process.pid}.txt`,
);
const helperFile = join(
  tmpdir(),
  `perry-child-process-large-output-${process.pid}.js`,
);
const fileContent = `${"x".repeat(40_000)}\n${"y".repeat(40_000)}\n`;

try {
  writeFileSync(dataFile, fileContent);
  writeFileSync(
    helperFile,
    "require('node:fs').createReadStream(process.argv[2]).pipe(process.stdout);",
  );
  const streamed = execFileSync("node", [helperFile, dataFile], {
    encoding: "utf8",
    maxBuffer: 1024 * 1024,
  });
  console.log("stream length:", streamed.length);
  console.log("stream exact:", streamed === fileContent);
  console.log(
    "stream boundary:",
    JSON.stringify(
      streamed[0] + streamed[40_000] + streamed[40_001] + streamed.at(-2),
    ),
  );
} finally {
  rmSync(dataFile, { force: true });
  rmSync(helperFile, { force: true });
}
