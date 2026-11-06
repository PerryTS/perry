"""Compare Perry's expansion with OpenTUI's real Babel transform under Bun.

Build `cargo build -p perry-hir --example solid_jsx --profile perry-dev`, then:
python compare.py /path/to/solid_jsx [--perry /path/to/perry] [--opencode /path/to/v1.18.30]
The optional corpus check expands and lowers every TSX source to HIR and runs
the upstream transform on the same inputs. It does not execute the full app.
"""

import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("expander", type=Path)
parser.add_argument("--perry", type=Path)
parser.add_argument("--opencode", type=Path)
args = parser.parse_args()
fixture = Path(__file__).resolve().parent
work = fixture / "work"
work.mkdir(exist_ok=True)
for name in ["package.json", "tsconfig.json", "host.ts", "main.tsx", "frames.tsx", "oracle.mjs"]:
    shutil.copy2(fixture / name, work / name)


def run(command, timeout=180, env=None):
    result = subprocess.run([str(arg) for arg in command], cwd=work, env=env,
                            capture_output=True, text=True, timeout=timeout)
    if result.returncode:
        raise RuntimeError(f"{command}\n{result.stdout}\n{result.stderr}")
    return result.stdout


run(["bun", "install", "--ignore-scripts"])
expander = args.expander.resolve()
expected = (fixture / "expected.txt").read_text()
for name, runtime in [("main", "./host.ts"), ("frames", "@opentui/solid")]:
    run(["bun", "oracle.mjs", f"{name}.tsx"])
    run([expander, f"{name}.tsx", runtime, f"{name}.expanded.ts"])
    oracle = run(["bun", "--conditions=browser", f"{name}.oracle.ts"])
    actual = run(["bun", "--conditions=browser", f"{name}.expanded.ts"])
    if actual != oracle:
        raise AssertionError(f"{name}: oracle={oracle!r}\nPerry={actual!r}")
    if name == "main" and actual != expected:
        raise AssertionError(f"unexpected fixture output: {actual!r}")
    print(f"PASS {name}: Perry expansion and OpenTUI transform agree", flush=True)

if args.perry:
    binary = work / ("main.exe" if os.name == "nt" else "main-bin")
    compiled = run([args.perry.resolve(), "compile", "--no-cache", "main.tsx", "-o", binary], timeout=600)
    if "5 native, 0 JavaScript" not in compiled:
        raise AssertionError(f"unexpected live graph (entry, host, Solid core/store/universal): {compiled}")
    actual = run([binary])
    if actual != expected:
        raise AssertionError(f"native output={actual!r}, expected={expected!r}")
    print("PASS native: universal renderer and reactive core/store", flush=True)

if args.opencode:
    root = args.opencode.resolve()
    revision = subprocess.check_output(["git", "-C", str(root), "rev-parse", "HEAD"], text=True).strip()
    if revision != "3104c1428ec91f809e5ab86631300de41eb6952e":
        raise AssertionError(f"expected OpenCode v1.18.30 (3104c14), got {revision}")
    files = sorted([*root.glob("packages/tui/src/**/*.tsx"), *root.glob("packages/opencode/src/**/*.tsx")])
    if len(files) < 107:
        raise AssertionError(f"incomplete OpenCode corpus: {len(files)} TSX files")
    # One Bun process runs the real transform for the entire corpus.
    (work / "corpus.json").write_text(json.dumps([str(path) for path in files]))
    (work / "corpus.mjs").write_text('''
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { createRequire } from "node:module";
import { pathToFileURL } from "node:url";
const require = createRequire(import.meta.url);
const plugin = require.resolve("@opentui/solid/bun-plugin");
const { transformSolidSource } = await import(pathToFileURL(join(dirname(plugin), "solid-transform.js")));
for (const filename of JSON.parse(readFileSync("corpus.json", "utf8"))) {
  await transformSolidSource(readFileSync(filename, "utf8"), { filename, moduleName: "@opentui/solid" });
}
''')
    run(["bun", "corpus.mjs"], timeout=600)
    for source in files:
        run([expander, source, "@opentui/solid", "corpus.expanded.ts"])
    print(f"PASS corpus: {len(files)} TSX files pass OpenTUI transform and Perry expansion/HIR", flush=True)
