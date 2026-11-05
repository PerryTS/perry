#!/usr/bin/env python3
"""Generate isolated young-object key-store witnesses into an output directory.

Run each with Node for parity and with a symbol-preserving Perry binary under
perf stat -e instructions:u,page-faults (three runs, median). Dynamic variants
perform the same three key selections even at zero added keys, so successive
instruction differences isolate the added stores. Constant variants show the
compiler's literal-key contrast. There are 2,000,000 fresh holders per file.
"""
from pathlib import Path
import argparse

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('output', type=Path)
args = parser.parse_args()
args.output.mkdir(parents=True, exist_ok=True)
for kind, value in [('num', 'i'), ('ptr', 'value')]:
    for added in range(4):
        source = '''const keys = ["runtimeAlpha", "runtimeBravo", "runtimeCharlie", "runtimeDelta"];
const value = {payload: 73};
let sink: any = null;
let key0: string = "", key1: string = "", key2: string = "";
for (let i = 0; i < 2000000; i++) {
 const o: any = {};
 const k0 = keys[i & 3], k1 = keys[(i + 1) & 3], k2 = keys[(i + 2) & 3];
'''
        for j in range(added):
            source += f' o[k{j}] = {value};\n'
        source += ''' sink = o; key0 = k0; key1 = k1; key2 = k2;
}
console.log(key0, key1, key2, JSON.stringify(sink));
'''
        (args.output / f'controlled-{kind}{added}.ts').write_text(source)
    for added in (1, 2, 3):
        source = '''const value = {payload: 73};
let sink: any = null;
for (let i = 0; i < 2000000; i++) {
 const o: any = {};
'''
        for j in range(added):
            source += f' o.constant{j} = {value};\n'
        source += ' sink = o;\n}\nconsole.log(JSON.stringify(sink));\n'
        (args.output / f'{kind}const{added}.ts').write_text(source)
