#!/usr/bin/env python3
"""Check the production validator/counter against std and count its allocations."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parent
REPO = ROOT.parent.parent


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--workdir', type=Path, default=ROOT / '.work/utf16-check')
    args = parser.parse_args()
    work = args.workdir.resolve()
    work.mkdir(parents=True, exist_ok=True)
    module = REPO / 'crates/perry-runtime/src/string/utf16_count.rs'
    source = (module.parent / 'mod.rs').read_text()
    start = source.index('pub(crate) fn compute_utf16_len_wtf8(')
    end = source.index('/// Finalize bytes', start)
    fallback = source[start:end]
    corpus = (ROOT / 'utf16_corpus.rs').read_text()
    # Import the exact production SIMD validator/counter; the existing WTF-8
    # fallback is copied unchanged to satisfy its parent-module reference.
    harness = '#[path = ' + json.dumps(str(module)) + '] mod counter;\n'
    (work / 'main.rs').write_text(harness + fallback + corpus)
    (work / 'Cargo.toml').write_text('''[package]
name = "perry-utf16-check"
version = "0.0.0"
edition = "2021"
[workspace]
[[bin]]
name = "utf16-check"
path = "main.rs"
[dependencies]
simdutf8 = "=0.1.5"
''')
    subprocess.run(['cargo', 'build', '--offline', '--release', '--manifest-path',
                    str(work / 'Cargo.toml')], check=True)
    result = subprocess.check_output([str(work / 'target/release/utf16-check')], text=True)
    record = {'counter_sha256': hashlib.sha256(module.read_bytes()).hexdigest(),
              'wtf8_fallback_sha256': hashlib.sha256(fallback.encode()).hexdigest(),
              'corpus_sha256': hashlib.sha256(corpus.encode()).hexdigest(),
              'result': result}
    (work / 'validation.json').write_text(json.dumps(record, indent=2) + '\n')
    print(result, end='')


if __name__ == '__main__':
    main()
