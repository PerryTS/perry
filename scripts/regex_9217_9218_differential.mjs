#!/usr/bin/env node

// Differential runner for #9217 / #9218.
//
// With no arguments it runs a focused corpus. To replay a bundle corpus, pass
// a UTF-8 TSV whose first field is the RegExp source and whose optional second
// field is its flags:
//
//   node scripts/regex_9217_9218_differential.mjs --corpus /tmp/regexes.tsv
//
// The runner deliberately emits one line per pattern. Its headline divergence
// count is therefore a count of divergent PATTERN RECORDS, not an inflated
// count of individual subject cells. Every RegExp is fresh for every subject,
// so `g` tests matching without carrying lastIndex into the next probe.

import { spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { basename, dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const scriptDir = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(scriptDir, "..");

function fail(message) {
  console.error(message);
  process.exit(2);
}

let corpusPath = null;
let perryPath = join(repoRoot, "target", "release", "perry");
for (let i = 2; i < process.argv.length; i += 1) {
  const arg = process.argv[i];
  if (arg === "--corpus") {
    corpusPath = process.argv[++i];
    if (!corpusPath) fail("--corpus needs a path");
  } else if (arg.startsWith("--corpus=")) {
    corpusPath = arg.slice("--corpus=".length);
  } else if (arg === "--perry") {
    perryPath = resolve(process.argv[++i] ?? "");
  } else if (arg.startsWith("--perry=")) {
    perryPath = resolve(arg.slice("--perry=".length));
  } else {
    fail(`unknown argument: ${arg}`);
  }
}

const subjects = [
  "Az_09-", // ASCII word/non-word
  "café", // accented Latin
  "Ωμέγα", // Greek
  "漢字", // CJK
  "😀", // astral emoji
  "K", // KELVIN SIGN: word only under i+u
  "ſ", // LATIN SMALL LETTER LONG S: word only under i+u
  "\n", // LF
  "\r", // CR
  " ", // LINE SEPARATOR
  " ", // PARAGRAPH SEPARATOR
  "\t\r\n", // tab + CRLF, including the issue #9218 repro
];

const focusedCases = [
  // Word escapes outside classes, with every relevant flag represented.
  ["^\\w+$", ""],
  ["^\\w+$", "i"],
  ["^\\w+$", "u"],
  ["^\\w+$", "iu"],
  ["^\\w+$", "g"],
  ["^\\w+$", "m"],
  ["^\\w+$", "s"],
  ["^\\w+$", "gimsu"],
  ["^\\W+$", ""],
  ["^\\W+$", "i"],
  ["^\\W+$", "u"],
  ["^\\W+$", "iu"],
  ["^\\W+$", "g"],
  ["\\b\\w+\\b", ""],
  ["\\b\\w+\\b", "i"],
  ["\\b\\w+\\b", "u"],
  ["\\b\\w+\\b", "iu"],
  ["\\b\\w+\\b", "gim"],
  ["\\B\\W+\\B", ""],
  ["\\B\\W+\\B", "iu"],
  ["x\\b.", ""],
  ["x\\B.", "m"],

  // Word escapes and the class meanings of \b / \B inside classes.
  ["^[\\w-]+$", ""],
  ["^[\\w-]+$", "i"],
  ["^[\\w-]+$", "u"],
  ["^[\\w-]+$", "iu"],
  ["^[^\\w]+$", ""],
  ["^[^\\w]+$", "i"],
  ["^[^\\w]+$", "iu"],
  ["^[\\W]+$", ""],
  ["^[\\W]+$", "i"],
  ["^[\\W]+$", "iu"],
  ["^[^\\W]+$", ""],
  ["^[^\\W]+$", "i"],
  ["^[^\\W]+$", "iu"],
  ["^[a\\w]+$", "i"],
  ["^[^a\\w]+$", "i"],
  ["^[a\\W]+$", "i"],
  ["^[^a\\W]+$", "i"],
  ["^[\\b]$", ""],
  ["^[\\b]$", "u"],
  ["^[\\B]$", ""],
  ["^[\\B]$", "i"],
  ["^[\\B]$", "u"], // SyntaxError in both engines

  // Dot without s excludes all four ECMAScript LineTerminators.
  ["^.$", ""],
  ["^.$", "i"],
  ["^.$", "u"],
  ["^.$", "m"],
  ["^.$", "g"],
  ["^.$", "s"],
  ["^.$", "is"],
  ["^.$", "gimsu"],
  [".{2}", ""],
  [".{2}", "g"],
  [".{2}", "s"],
  [".{2}", "gs"],

  // #9216: keep the cheap dotAll/empty-intersection translations intact.
  ["[^]", ""],
  ["[^]", "i"],
  ["[^]", "u"],
  ["[^]", "gimsu"],
  ["[]", ""],
  ["[]", "i"],
  ["[]", "u"],
  ["[]", "gimsu"],
  ["[\\s\\S]", "i"],
  ["[\\w\\W]", "i"],
  ["[^\\w\\W]", "i"],
];

function readCorpus(path) {
  const lines = readFileSync(path, "utf8").split(/\r?\n/u);
  const cases = [];
  for (const line of lines) {
    if (line.length === 0 || line.startsWith("#")) continue;
    const tab = line.indexOf("\t");
    cases.push(tab === -1 ? [line, ""] : [line.slice(0, tab), line.slice(tab + 1)]);
  }
  return cases;
}

const cases = corpusPath ? readCorpus(resolve(corpusPath)) : focusedCases;
if (cases.length === 0) fail("corpus is empty");

const temp = mkdtempSync(join(tmpdir(), "perry-regex-diff-"));
const sourcePath = join(temp, "probe.ts");
const nativePath = join(temp, "probe");
const source = `
const cases: string[][] = ${JSON.stringify(cases)};
const subjects: string[] = ${JSON.stringify(subjects)};

for (const entry of cases) {
  const pattern = entry[0];
  const flags = entry[1];
  const answers: string[] = [];
  for (const subject of subjects) {
    try {
      const re = new RegExp(pattern, flags);
      answers.push(re.test(subject) ? "1" : "0");
    } catch (error) {
      answers.push(error instanceof SyntaxError ? "S" : "E");
    }
  }
  console.log(JSON.stringify([pattern, flags, answers.join("")]));
}
`;
writeFileSync(sourcePath, source);

function run(command, args, options = {}) {
  const result = spawnSync(command, args, {
    cwd: repoRoot,
    encoding: "utf8",
    maxBuffer: 64 * 1024 * 1024,
    ...options,
  });
  if (result.error) fail(`${command}: ${result.error.message}`);
  if (result.status !== 0) {
    fail(
      `${command} exited ${result.status}\n${result.stdout ?? ""}${result.stderr ?? ""}`,
    );
  }
  return result.stdout;
}

try {
  const nodeOutput = run(process.execPath, ["--experimental-strip-types", sourcePath]);
  run(perryPath, ["compile", sourcePath, "-o", nativePath], {
    env: {
      ...process.env,
      PERRY_NO_CACHE: "1",
      PERRY_RUNTIME_DIR: join(repoRoot, "target", "release"),
    },
  });
  const perryOutput = run(nativePath, []);

  const nodeLines = nodeOutput.trimEnd().split("\n");
  const perryLines = perryOutput.trimEnd().split("\n");
  const divergent = [];
  const subjectDivergences = new Array(subjects.length).fill(0);
  const count = Math.max(nodeLines.length, perryLines.length);
  for (let i = 0; i < count; i += 1) {
    if (nodeLines[i] === perryLines[i]) continue;
    divergent.push(i);
    if (nodeLines[i] !== undefined && perryLines[i] !== undefined) {
      const nodeRecord = JSON.parse(nodeLines[i]);
      const perryRecord = JSON.parse(perryLines[i]);
      const nodeAnswers = nodeRecord[2];
      const perryAnswers = perryRecord[2];
      for (let j = 0; j < subjects.length; j += 1) {
        if (nodeAnswers[j] !== perryAnswers[j]) subjectDivergences[j] += 1;
      }
    }
  }

  console.log(
    `patterns=${cases.length} subjects=${subjects.length} divergent_records=${divergent.length}`,
  );
  console.log(`subject_cells=${subjectDivergences.reduce((a, b) => a + b, 0)}`);
  for (let i = 0; i < subjects.length; i += 1) {
    console.log(`subject[${i}]=${JSON.stringify(subjects[i])} divergences=${subjectDivergences[i]}`);
  }
  for (const index of divergent.slice(0, 100)) {
    console.log(`DIFF ${index + 1} node=${nodeLines[index]}`);
    console.log(`DIFF ${index + 1} perry=${perryLines[index]}`);
  }
  if (divergent.length > 100) {
    console.log(`... ${divergent.length - 100} additional divergent records omitted`);
  }
  process.exitCode = divergent.length === 0 ? 0 : 1;
} finally {
  rmSync(temp, { recursive: true, force: true });
}

