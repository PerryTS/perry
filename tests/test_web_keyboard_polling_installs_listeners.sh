#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
WASM_RUNTIME="$ROOT/crates/perry-codegen-wasm/src/wasm_runtime.js"
JS_RUNTIME="$ROOT/crates/perry-codegen-js/src/web_runtime.js"

node - "$WASM_RUNTIME" "$JS_RUNTIME" <<'NODE'
const fs = require('fs');
const [wasmRuntime, jsRuntime] = process.argv.slice(2);

function functionBody(source, name) {
  const re = new RegExp(`function ${name.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}\\([^)]*\\) \\{`);
  const match = re.exec(source);
  if (!match) throw new Error(`missing ${name}`);

  let depth = 1;
  let i = match.index + match[0].length;
  const start = i;
  while (i < source.length && depth > 0) {
    if (source[i] === '{') depth += 1;
    else if (source[i] === '}') depth -= 1;
    i += 1;
  }
  if (depth !== 0) throw new Error(`unterminated ${name}`);
  return source.slice(start, i - 1);
}

function assertContains(haystack, needle, label) {
  if (!haystack.includes(needle)) {
    throw new Error(`${label} must call ${needle}`);
  }
}

const wasm = fs.readFileSync(wasmRuntime, 'utf8');
assertContains(functionBody(wasm, 'perry_ui_is_key_down'), '__perryEnsureKbdInstalled();', 'wasm isKeyDown');
assertContains(functionBody(wasm, 'perry_ui_current_modifiers'), '__perryEnsureKbdInstalled();', 'wasm currentModifiers');

const js = fs.readFileSync(jsRuntime, 'utf8');
assertContains(functionBody(js, 'perry_ui_is_key_down'), '_perryEnsureKbd();', 'js isKeyDown');
assertContains(functionBody(js, 'perry_ui_current_modifiers'), '_perryEnsureKbd();', 'js currentModifiers');
NODE
