//! Rule M3: a string with a known constant prefix (ECMA-262 13.15.3: `c + x`
//! with `c` a String is `c` followed by ToString of `x`, never preceded by
//! it) is a different property key from a string whose known prefix is
//! incompatible: two strings are equal only when one's known prefix is a
//! prefix of the other's (7.2.13 / 7.2.14 on Strings). A store under such
//! a key writes only the names its prefix admits; a read reads only those,
//! the compatible prefixes and what an unknown key wrote; a prototype member
//! only when its name starts with the prefix (`'a:' + i` never names
//! `toString`). three.js GLTFLoader `GLTFRegistry` (`this.objects[key]`,
//! `cacheKey = type + ':' + index`): every glTF object flowed merged.
//! Controls: an unknown prefix (`process.argv[2]`), a store under an
//! unknown key, compatible prefixes (`'c' + i` and `'cd' + i`), a read
//! under `'to' + process.argv[2]` (may be `toString`).
use super::*;

/// The sorted class names (or other tokens) the `.p` reads see.
fn recv(pf: &ProgramFacts) -> Vec<String> {
    let mut r: Vec<String> = pf
        .sites
        .iter()
        .filter(|s| s.key == "p" && !s.is_call)
        .flat_map(|s| s.receivers.iter())
        .map(|r| r.rsplit(':').next().unwrap_or(r).to_string())
        .collect();
    r.sort();
    r.dedup();
    r
}

fn prog(body: &str) -> String {
    format!(
        "class A {{ p = 1; }}
         class B {{ p = 2; }}
         const reg: any = {{}};
         const n = process.argv.length;
         {body}
         console.log(x.p);"
    )
}

const FUNS: &str = "function put(t: any, i: any, v: any) { reg[t + ':' + i] = v; }
         function get(t: any, i: any) { return reg[t + ':' + i]; }";

fn only_a(body: &str) -> Vec<String> {
    recv(&facts(&prog(body)))
}

#[test]
fn literal_prefix_keys_are_apart() {
    let v = only_a("reg['c:' + n] = new A(); reg['d:' + n] = new B(); const x: any = reg['c:' + n];");
    assert_eq!(v, vec!["A"], "{v:?}");
}

#[test]
fn local_prefix_keys_are_apart() {
    let v = only_a(
        "const k1 = 'c:' + n; reg[k1] = new A(); const k2 = 'd:' + n; reg[k2] = new B();
         const k3 = 'c:' + n; const x: any = reg[k3];",
    );
    assert_eq!(v, vec!["A"], "{v:?}");
}

#[test]
fn concatenated_constant_prefix_keys_are_apart() {
    let v = only_a(
        "const c = 'c'; const d = 'd'; const k1 = c + ':' + n; reg[k1] = new A();
         const k2 = d + ':' + n; reg[k2] = new B(); const k3 = c + ':' + n; const x: any = reg[k3];",
    );
    assert_eq!(v, vec!["A"], "{v:?}");
}

#[test]
fn registry_functions_split_by_prefix() {
    let v = only_a(&format!(
        "{FUNS}\n put('a', n, new A()); put('b', n, new B()); const x: any = get('a', n);"
    ));
    assert_eq!(v, vec!["A"], "{v:?}");
}

#[test]
fn control_unknown_prefix_merges() {
    let v = only_a(&format!(
        "{FUNS}\n put('a', n, new A()); put(process.argv[2], n, new B()); const x: any = get('a', n);"
    ));
    assert!(v.contains(&"A".into()) && v.contains(&"B".into()), "{v:?}");
    let v = only_a(
        "const k1 = 'c:' + n; reg[k1] = new A(); const k2 = process.argv[2] + ':' + n; reg[k2] = new B();
         const k3 = 'c:' + n; const x: any = reg[k3];",
    );
    assert!(v.contains(&"A".into()) && v.contains(&"B".into()), "{v:?}");
}

#[test]
fn control_unknown_key_store_merges() {
    let v = only_a(
        "const k1 = 'c:' + n; reg[k1] = new A(); reg[process.argv[2]] = new B();
         const k3 = 'c:' + n; const x: any = reg[k3];",
    );
    assert!(v.contains(&"A".into()) && v.contains(&"B".into()), "{v:?}");
}

#[test]
fn control_compatible_prefixes_merge() {
    let v = only_a(
        "const k1 = 'c' + n; reg[k1] = new A(); const k2 = 'cd' + n; reg[k2] = new B();
         const k3 = 'c' + n; const x: any = reg[k3];",
    );
    assert!(v.contains(&"A".into()) && v.contains(&"B".into()), "{v:?}");
}

#[test]
fn prefix_read_names_a_member_only_under_its_prefix() {
    // `'c:' + n` never spells an Object.prototype member: only A.
    let v = only_a("const k1 = 'c:' + n; reg[k1] = new A(); const k3 = 'c:' + n; const x: any = reg[k3];");
    assert_eq!(v, vec!["A"], "{v:?}");
    // `'to' + s` may be `toString`: the read holds Object.prototype members.
    let v = only_a(
        "const k1 = 'to' + n; reg[k1] = new A(); const k3 = 'to' + process.argv[2]; const x: any = reg[k3];",
    );
    assert!(v.len() > 1, "control: 'to' + s may name toString: {v:?}");
}
