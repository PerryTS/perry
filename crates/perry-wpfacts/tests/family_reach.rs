//! Soundness probes of `family_reach`: programs in which a typed array or a
//! Buffer exists at run time, or in which one of the family's globals is
//! observed, must have the typed-array family live. Each probe is one of
//! the "must say may" cases of DESIGN-family-reach (the review probes of
//! REVIEW-family-reach-bufeq.md and the arrivals without a mention).
use perry_wpfacts::{FactsConfig, FamilyReach};

fn reach(src: &str) -> FamilyReach {
    let path = "main.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "main", &path).expect("lowers");
    let list: Vec<(String, &perry_hir::Module)> = vec![(path, &hir)];
    let mut cfg = FactsConfig::default();
    cfg.max_work.get_or_insert(50_000_000);
    perry_wpfacts::compute(&list, &cfg)
        .family_reach
        .expect("the fact is computed for a whole program")
}

/// (name, source): a byte cell is born, arrives or is observed.
const LIVE: &[(&str, &str)] = &[
    ("new", "const a = new Uint8Array(4);\na[1] = 7;\nconsole.log(a.length, a[1]);\n"),
    ("buffer-from", "console.log(Buffer.from('ab').length);\n"),
    ("typeof", "console.log(typeof Uint8Array);\n"),
    ("pool-size", "console.log(Buffer.poolSize);\n"),
    ("const-key", "const C = (globalThis as any)['Uint8' + 'Array'];\nconsole.log(typeof C);\n"),
    (
        "unknown-key",
        "const k = process.argv[2] ?? 'Uint8Array';\nconst C = (globalThis as any)[k];\nconsole.log(new C(3).length);\n",
    ),
    (
        "for-in-global",
        "let n = 0;\nfor (const k in globalThis) { const v = (globalThis as any)[k]; if (typeof v === 'function') n++; }\nconsole.log(n > 0);\n",
    ),
    ("global-values", "console.log(Object.values(globalThis).length > 0);\n"),
    ("global-entries", "console.log(Object.entries(globalThis).length > 0);\n"),
    (
        "global-descriptors",
        "console.log(Object.keys(Object.getOwnPropertyDescriptors(globalThis)).length > 0);\n",
    ),
    (
        "unwalked-callback",
        "let n = 0;\n[4].forEach(...[(x: number) => { n = new Uint8Array(x).length; }]);\nconsole.log(n);\n",
    ),
    ("text-encoder", "console.log(new TextEncoder().encode('abc').length);\n"),
    ("structured-clone", "const c = structuredClone({ a: [1, 2] });\nconsole.log(c.a.length);\n"),
    (
        "response-body",
        "new Response('ab').arrayBuffer().then((b) => console.log(b.byteLength));\n",
    ),
    (
        "wasm-memory",
        "const m = new WebAssembly.Memory({ initial: 1 });\nconsole.log(m.buffer.byteLength);\n",
    ),
    (
        "fs-callback",
        "import * as fs from 'fs';\nfs.readFile('/etc/hostname', (e, d) => { console.log(d.length > 0); });\n",
    ),
    (
        "stream-data",
        "import { Readable } from 'stream';\nconst r = new Readable({ read() { this.push('ab'); this.push(null); } });\nr.on('data', (c) => console.log(c.length));\n",
    ),
    (
        "native-module",
        "import * as zlib from 'zlib';\nconsole.log(zlib.gzipSync('aaaa').length > 0);\n",
    ),
    (
        "stdin-chunks",
        "let n = 0;\nprocess.stdin.on('data', (c) => { n += c.length; });\nprocess.stdin.on('end', () => console.log(n));\n",
    ),
    (
        "worker-message",
        "import { MessageChannel } from 'worker_threads';\nconst { port1, port2 } = new MessageChannel();\nport2.on('message', (m) => { console.log(m.length); port2.close(); });\nport1.postMessage(new Uint8Array(3));\n",
    ),
    (
        "typed-parameter",
        "function g(a: Float64Array | null): boolean { return a === null; }\nconsole.log(g(null));\n",
    ),
    (
        "crypto-random",
        "const b = new Uint8Array(4);\ncrypto.getRandomValues(b);\nconsole.log(b.length);\n",
    ),
    ("data-view", "const v = new DataView(new ArrayBuffer(8));\nconsole.log(v.byteLength);\n"),
];

/// (name, source): no byte cell exists and none of the family's entries is
/// observed (the sort scratch cells are no JavaScript values).
const NEVER: &[(&str, &str)] = &[
    (
        "sort-scratch",
        "const a: number[] = [];\nfor (let i = 0; i < 100; i++) a.push((i * 7919) % 101);\na.sort((x, y) => x - y);\nconsole.log(a[0], a[99]);\n",
    ),
    ("global-keys", "console.log(Object.keys(globalThis).length >= 0);\n"),
    (
        "json-reviver",
        "const v = JSON.parse('{\"type\":\"Buffer\",\"data\":[1,2,3]}', function (k, x) { return typeof x === 'number' ? x + 1 : x; });\nconsole.log(v.type, v.data.length);\n",
    ),
    ("argv", "console.log(process.argv.length);\n"),
];

#[test]
fn a_program_with_a_byte_cell_has_the_typed_array_family_live() {
    for (name, src) in LIVE {
        let r = reach(src);
        assert!(r.live("typedarray"), "{name}: {:#?}", r.dump_lines());
    }
}

#[test]
fn a_program_without_a_byte_cell_can_have_the_family_never() {
    for (name, src) in NEVER {
        let r = reach(src);
        assert!(!r.live("typedarray"), "{name}: {:#?}", r.dump_lines());
    }
}
