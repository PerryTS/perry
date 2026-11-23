// R9BIRTH8 (2026-11-16): a typed array's named members after a `.buffer`
// read (B1: bytes only) and after a store under a getter-only key.
use super::*;
use perry_hir::Module;

fn hirs(src: &str) -> Vec<(String, Module)> {
    let path = "a.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "a", &path).expect("lowers");
    vec![(path, hir)]
}

fn facts(src: &str) -> crate::ProgramFacts {
    let h = hirs(src);
    let list: Vec<(String, &Module)> = h.iter().map(|(p, m)| (p.clone(), m)).collect();
    let cfg = FactsConfig {
        unit_io: true,
        ..Default::default()
    };
    crate::compute(&list, &cfg)
}

fn sites(pf: &crate::ProgramFacts, key: &str) -> Vec<String> {
    let mi = pf
        .module_paths
        .iter()
        .position(|p| p == "a.ts")
        .expect("a.ts") as u32;
    let v: Vec<_> = pf
        .sites
        .iter()
        .filter(|s| s.module == mi && s.key == key && !s.access.is_store())
        .map(|s| format!("{:?}", s.verdict))
        .collect();
    for verdict in &v {
        eprintln!("RB8 .{key} verdict={verdict}");
    }
    v
}

fn all(pf: &crate::ProgramFacts, key: &str, want: &str) -> bool {
    let v = sites(pf, key);
    !v.is_empty() && v.iter().all(|x| x == want)
}

fn none(pf: &crate::ProgramFacts, key: &str, want: &str) -> bool {
    let v = sites(pf, key);
    !v.is_empty() && v.iter().all(|x| x != want)
}

// ------------------------------------------------------- 1: typed arrays

const TA: &str = "const n = Number(process.argv[2]) || 3;
const ta = new Float32Array(n);
BODY
console.log(ta.length);
";

fn ta(body: &str) -> crate::ProgramFacts {
    facts(&TA.replace("BODY", body))
}

// T1 (RED at rb7): a `.buffer` read exposes the bytes (B1), never a named
// key of the view (10.4.5.2-5): `length` stays %TypedArray%.prototype's.
#[test]
fn r9birth8_t1_buffer_read_keeps_length() {
    let pf = ta("const b = ta.buffer; console.log(b.byteLength, new Uint8Array(b).length);");
    assert!(all(&pf, "length", "NativeMember"), "T1");
}

// TK1: a `.buffer` read and an own `length` definition, in either order.
#[test]
fn r9birth8_tk1_buffer_and_define() {
    let pf = ta("const b = ta.buffer; Object.defineProperty(ta, 'length', { value: 1 }); console.log(b.byteLength);");
    assert!(none(&pf, "length", "NativeMember"), "TK1 buffer first");
    let pf = ta("Object.defineProperty(ta, 'length', { value: 1 }); const b = ta.buffer; console.log(b.byteLength);");
    assert!(none(&pf, "length", "NativeMember"), "TK1 define first");
}

// TK2 (RED at rb7): `ta.length = 5` (10.4.5.5 -> OrdinarySet: the
// inherited accessor has no setter, 10.1.9.2: false, a TypeError in a
// strict module; node prints the catch): no own `length`, the read stays
// the builtin's.
#[test]
fn r9birth8_tk2_stores_keep_length() {
    let pf = ta("const b = ta.buffer; try { (ta as any).length = 5; } catch (e) { console.log('TypeError', e instanceof TypeError); } console.log(b.byteLength);");
    assert!(all(&pf, "length", "NativeMember"), "TK2 named store");
}

// TK3: the view handed to unknown code may be redefined there.
#[test]
fn r9birth8_tk3_view_to_unknown() {
    let pf = ta("const b = ta.buffer; const u: any = (globalThis as any)[String(Math.random())]; if (u) u(ta); console.log(b.byteLength);");
    assert!(none(&pf, "length", "NativeMember"), "TK3");
}
