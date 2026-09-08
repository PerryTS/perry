// Standalone optimizer regression. Requires only the repo's Rust toolchain;
// no Claude Code source, runtime archives, network, credentials, or long build.
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const source = fs.readFileSync(path.join(root, 'crates/perry-ext-net/src/dispatch.rs'), 'utf8');
const adapter = source.match(/(?:#\[inline\(never\)\]\n)?extern "C" fn process_pending_aux\(\) -> i32 \{[\s\S]*?\n\}/)?.[0];
assert(adapter, 'Cannot locate the actual network pump adapter');
const work = fs.mkdtempSync(path.join(os.tmpdir(), 'perry-net-pump-identity-'));
let passed = false;
try {
  function compile(callback, opt, tag) {
    const input = path.join(work, `${tag}.rs`);
    const output = path.join(work, `${tag}.ll`);
    // Mirror the legacy exported wrapper too: identical bodies are what make
    // LLVM rewrite the private callback to an interposable public symbol.
    fs.writeFileSync(input, `
unsafe extern "C" {
    fn js_ext_net_drain_pending() -> i32;
    fn register_pump(callback: extern "C" fn() -> i32);
}
${callback}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn js_net_process_pending() -> i32 {
    unsafe { js_ext_net_drain_pending() }
}
#[unsafe(no_mangle)]
pub extern "C" fn install_probe() {
    unsafe { register_pump(process_pending_aux) }
}
`, { flag: 'wx' });
    const result = spawnSync('rustc', ['--crate-type', 'lib', '-C', `opt-level=${opt}`,
      '--emit', 'llvm-ir', input, '-o', output],
    { cwd: root, encoding: 'utf8', timeout: 30_000 });
    assert.equal(result.status, 0, result.error?.message ?? result.stderr);
    return fs.readFileSync(output, 'utf8');
  }
  function registeredSymbol(ir) {
    const install = ir.match(/^define[^\n]*@install_probe\([^\n]*\n[\s\S]*?^}/m)?.[0];
    assert(install, 'Missing registration function');
    const symbol = install.match(/call void @register_pump\(ptr[^\n]*@([^ )]+)\)/)?.[1];
    assert(symbol, 'Missing callback registration operand');
    return symbol;
  }
  for (const opt of ['0', '1', '2', '3', 's', 'z']) {
    const ir = compile(adapter, opt, `fixed_${opt}`);
    const symbol = registeredSymbol(ir);
    assert(symbol.includes('process_pending_aux'), `-O${opt} registered ${symbol}, not the private adapter`);
    assert.notEqual(symbol, 'js_net_process_pending');
    console.log(`PASS: network pump retains private identity at Rust -O${opt}`);
  }
  const unprotected = adapter.replace('#[inline(never)]\n', '');
  assert.notEqual(unprotected, adapter, 'The merge protection must be present');
  assert.equal(registeredSymbol(compile(unprotected, '3', 'unprotected')),
    'js_net_process_pending', 'Negative control did not reproduce the original aliasing bug');
  console.log('PASS: negative control reproduces the legacy-symbol substitution');
  passed = true;
} finally {
  if (passed) fs.rmSync(work, { recursive: true });
  else console.error(`Preserved failing optimizer artifacts: ${work}`);
}
