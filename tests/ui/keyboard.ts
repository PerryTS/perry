import { readFileSync } from "node:fs";
import { resolve } from "node:path";

const root = resolve(import.meta.dirname, "../..");

function body(source: string, name: string): string {
  const escaped = name.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const match = new RegExp(`function ${escaped}\\([^)]*\\) \\{`).exec(source);
  if (!match) throw new Error(`missing ${name}`);

  let depth = 1;
  let i = match.index + match[0].length;
  const start = i;
  while (i < source.length && depth > 0) {
    if (source[i] === "{") depth += 1;
    else if (source[i] === "}") depth -= 1;
    i += 1;
  }
  if (depth !== 0) throw new Error(`unterminated ${name}`);
  return source.slice(start, i - 1);
}

function includes(source: string, needle: string, label: string): void {
  if (!source.includes(needle)) throw new Error(`${label} must call ${needle}`);
}

const wasm = readFileSync(resolve(root, "crates/perry-codegen-wasm/src/wasm_runtime.js"), "utf8");
includes(body(wasm, "perry_ui_is_key_down"), "__perryEnsureKbdInstalled();", "wasm isKeyDown");
includes(body(wasm, "perry_ui_current_modifiers"), "__perryEnsureKbdInstalled();", "wasm currentModifiers");

const js = readFileSync(resolve(root, "crates/perry-codegen-js/src/web_runtime.js"), "utf8");
includes(body(js, "perry_ui_is_key_down"), "_perryEnsureKbd();", "js isKeyDown");
includes(body(js, "perry_ui_current_modifiers"), "_perryEnsureKbd();", "js currentModifiers");
