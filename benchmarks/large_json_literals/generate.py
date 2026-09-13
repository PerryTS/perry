"""Generate the #10161 typed hot-loop and record-array codegen probes.

Usage: python3 benchmarks/large_json_literals/generate.py /tmp/literal-probes
This only writes TypeScript; compile/run it with the compiler being measured.
"""

import json
from pathlib import Path
import sys


def records(count):
    return [dict(id=i, name=f"n{i}", tags=["a", f"b{i % 5}"], w=i / 4)
            for i in range(count)]


def table():
    values = []
    for i in range(2000):
        n = (i * 427799) % 1000003 - 500000
        values.append(n + (0.5 if n >= 0 else -0.5) if i % 7 == 0 else n)
    return values


REC_TYPE = "type Rec = { id: number; name: string; tags: string[]; w: number };\n"
HOT_LOOP = """
let t0 = performance.now();
let s = 0;
for (let r = 0; r < 20000; r++) { for (let i = 0; i < table.length; i++) { s += table[i] * (i & 3); } }
const t1 = performance.now();
let w = 0;
for (let r = 0; r < 20000; r++) { for (let j = 0; j < recs.length; j++) { const q = recs[j]; w += q.w + q.tags.length + q.id; } }
const t2 = performance.now();
console.log("table_ms", Math.round(t1 - t0), "recs_ms", Math.round(t2 - t1), s, w);
"""


def main():
    root = Path(sys.argv[1])
    root.mkdir(parents=True, exist_ok=True)
    source = ("const table: number[] = " + json.dumps(table()) + ";\n" + REC_TYPE
              + "const recs: Rec[] = " + json.dumps(records(400)) + ";\n" + HOT_LOOP)
    (root / "hot.ts").write_text(source)
    # Separate entrypoints distinguish representation cost from the existing
    # whole-function LLVM size guards triggered by another literal in main.
    numbers = ("const table: number[] = " + json.dumps(table()) + ";\n"
               + HOT_LOOP.split("const t1 =")[0]
               + 'console.log("table_ms", Math.round(performance.now() - t0), s);\n')
    (root / "numbers.ts").write_text(numbers)
    record_hot = (REC_TYPE + "const recs: Rec[] = " + json.dumps(records(400)) + ";\n"
                  + "const t1 = performance.now();\n"
                  + HOT_LOOP.split("const t1 = performance.now();", 1)[1].split("console.log(")[0]
                  + 'console.log("recs_ms", Math.round(t2 - t1), w);\n')
    (root / "records.ts").write_text(record_hot)
    for count in [400, 1600, 3200, 4800, 6400, 12800]:
        data = records(count)
        literal = json.dumps(data)
        source = (REC_TYPE + "const recs: Rec[] = " + literal + ";\n"
                  + "let w = 0;\n"
                  + "for (let j = 0; j < recs.length; j++) { const q = recs[j]; w += q.w + q.tags.length + q.id; }\n"
                  + "console.log(recs.length, w);\n")
        (root / f"records-{count}.ts").write_text(source)
        text_bytes = sum(11 + len(r["name"]) + sum(map(len, r["tags"])) for r in data)
        print(f"records={count} source_bytes={len(literal)} "
              f"value_nodes={1 + 7 * count} key_string_bytes={text_bytes}")


if __name__ == "__main__":
    main()
