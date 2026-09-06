#!/usr/bin/env python3
"""Compare the actual JSON numeric emitter with Node/Bun, without float parsing.

The leaf harness extracts write_number verbatim from the runtime. Only the
BigInt callback is stubbed (the corpus contains finite doubles); full runtime
tests cover tagged values and callback behavior separately. A counting allocator
also checks that formatting into a pre-reserved output makes no allocations.
"""
import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess

ROOT = Path(__file__).resolve().parent
REPO = ROOT.parent.parent


def function(source, name):
    start = source.index('pub(crate) unsafe fn ' + name + '(')
    end = source.index('\n#[inline]', start)
    return source[start:end]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--node', default='node')
    parser.add_argument('--bun', default='bun')
    parser.add_argument('--count', type=int, default=1_000_000)
    parser.add_argument('--workdir', type=Path, default=ROOT / '.work/number-check')
    args = parser.parse_args()
    work = args.workdir.resolve()
    work.mkdir(parents=True, exist_ok=True)
    records = {}
    for name, engine in [('node', args.node), ('bun', args.bun)]:
        output = work / (name + '.tsv')
        with output.open('wb') as stream:
            subprocess.run([engine, str(ROOT / 'number_corpus.mjs'), str(args.count)],
                           stdout=stream, check=True)
        records[name] = {
            'version': subprocess.check_output([engine, '--version'], text=True).strip(),
            'sha256': hashlib.sha256(output.read_bytes()).hexdigest(),
        }
    assert records['node']['sha256'] == records['bun']['sha256'], 'Oracle disagreement'
    source = (REPO / 'crates/perry-runtime/src/json/stringify_scalars.rs').read_text()
    emitter = function(source, 'write_number')
    tags = (REPO / 'crates/perry-runtime/src/value/tags.rs').read_text()
    constants = '\n'.join(re.search(r'pub\(crate\) const ' + name + r': u64 = [^;]+;', tags)[0]
                          for name in ['BIGINT_TAG', 'INT32_TAG', 'INT32_MASK'])
    prefix = '''
use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::io::BufRead;
static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);
struct Counted;
unsafe impl GlobalAlloc for Counted {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        System.alloc(layout)
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) { System.dealloc(ptr, layout) }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        System.realloc(ptr, layout, size)
    }
}
#[global_allocator]
static ALLOCATOR: Counted = Counted;
mod builtins { pub const INT_EXACT_FASTPATH_LIMIT: f64 = 9_007_199_254_740_992.0; }
unsafe fn serialize_bigint(_: f64, _: &mut String) { panic!("non-finite corpus entry") }
'''
    main_rs = '''
fn main() {
    let file = std::fs::File::open(std::env::args().nth(1).unwrap()).unwrap();
    let mut output = String::with_capacity(64);
    let mut count = 0;
    let mut old_differences = Vec::new();
    for line in std::io::BufReader::new(file).lines() {
        let line = line.unwrap();
        let (bits, expected) = line.split_once('\\t').unwrap();
        let bits = u64::from_str_radix(bits, 16).unwrap();
        let value = f64::from_bits(bits);
        assert!(value.is_finite());
        output.clear();
        let before = ALLOCATIONS.load(Ordering::Relaxed);
        unsafe { write_number(&mut output, value) };
        assert_eq!(before, ALLOCATIONS.load(Ordering::Relaxed), "allocated: {bits:016x}");
        assert_eq!(output, expected, "bits={bits:016x}");
        // Capture useful regression vectors where the old Rust Display path
        // (used in this magnitude range) chose a different last digit.
        if old_differences.len() < 12 && (1e-6..1e21).contains(&value.abs())
            && !(value.fract() == 0.0 && value.abs() < builtins::INT_EXACT_FASTPATH_LIMIT) {
            let old = value.to_string();
            if old != expected { old_differences.push(format!("{bits:016x} {old} -> {expected}")); }
        }
        count += 1;
    }
    println!("PASS {count} exact number spellings; zero temporary allocations");
    for row in old_differences { println!("OLD_DIFFERENCE {row}"); }
}
'''
    (work / 'main.rs').write_text(prefix + constants + '\n' + emitter + main_rs)
    (work / 'Cargo.toml').write_text('''[package]
name = "perry-json-number-check"
version = "0.0.0"
edition = "2021"
[workspace]
[[bin]]
name = "number-check"
path = "main.rs"
[dependencies]
ryu-js = "=1.0.3"
itoa = "1.0"
''')
    subprocess.run(['cargo', 'build', '--offline', '--release', '--manifest-path',
                    str(work / 'Cargo.toml')], check=True)
    result = subprocess.check_output([str(work / 'target/release/number-check'),
                                      str(work / 'node.tsv')], text=True)
    records['emitter_sha256'] = hashlib.sha256(emitter.encode()).hexdigest()
    records['result'] = result
    (work / 'validation.json').write_text(json.dumps(records, indent=2) + '\n')
    print(result, end='')


if __name__ == '__main__':
    main()
