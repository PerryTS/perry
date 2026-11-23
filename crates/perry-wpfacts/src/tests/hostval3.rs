//! HOSTVAL3: realm members the runtime source settles read their declared
//! value, never ⊤ (each case cites the declaration it reads).
use super::*;
use crate::model::prim;

/// The kind of `H.v` after `h.v = <read>`.
fn stored(read: &str) -> crate::derive::KindFact {
    let pf = facts(&format!(
        "class H {{ v: any = 0; }}\nconst h = new H();\nh.v = {read};\nconsole.log(h.v);\n"
    ));
    field(tmpl(&pf, "H"), "v").kind.clone()
}

fn assert_prim(read: &str, p: u16) {
    let k = stored(read);
    assert!(!k.top, "{read}: {k:?}");
    assert_eq!(k.prims & p, p, "{read}: {k:?}");
}

/// `target(process.arch, string)` (native_module.rs:860, process.rs:860-861).
#[test]
fn process_arch_and_platform_are_strings() {
    assert_prim("process.arch", prim::STR);
    assert_prim("process.platform", prim::STR);
}

/// `Set.prototype.size.<get>` returns a number (collection_proto_thunks.rs
/// :567-573), also on the realm-born `process.allowedNodeEnvironmentFlags`
/// (HOSTNS: a held realm namespace reads the members of its declared chain).
#[test]
fn allowed_flags_size_is_a_number() {
    assert_prim("process.allowedNodeEnvironmentFlags.size", prim::NUM);
}

/// `Crypto.prototype.subtle.<get>` returns the one `crypto.subtle`
/// namespace (ctor_thunks.rs:401-407, native_module.rs:927-930).
#[test]
fn crypto_subtle_is_described() {
    let k = stored("(globalThis as any).crypto.subtle");
    assert!(!k.top, "{k:?}");
}

/// `Map.prototype.size.<get>` (collection_proto_thunks.rs:678-685) and the
/// %TypedArray% length getters (typed_array.rs:504-540) return numbers.
#[test]
fn map_size_and_typed_array_lengths_are_numbers() {
    assert_prim("(new Map() as any).size", prim::NUM);
    for k in ["length", "byteLength", "byteOffset"] {
        assert_prim(&format!("(new Float32Array(4) as any).{k}"), prim::NUM);
    }
}
