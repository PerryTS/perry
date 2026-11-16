//! SKINROOT14 (skin c1343 / r8 c1323, three.core.js:55444/55449
//! `knownActions.push( action )` in `AnimationMixer::_addInactiveAction`):
//! the callee set holds `ArrayBuffer.prototype.{resize, slice, transfer,
//! transferToFixedLength}` and the receiver is a typed array
//! (`indexed-owner@18`, BObj TypedArray).
//! (a) GLTFLoader.js:2345 `objectDef.extensions[ name ]` of `for (const name
//! in objectDef.extensions)` on an ArrayBuffer subject (three.core.js
//! `ArrayBuffer#84/#86`) read every ArrayBuffer.prototype method: for-in
//! over an ArrayBuffer, SharedArrayBuffer or DataView visits only keys the
//! program wrote (no own property: 25.1.3.1, 25.2.2.1, 25.3.2.1; prototype
//! members non-enumerable, clause 18; 14.7.5.9).
//! (b) a brand-checked ArrayBuffer method called on a typed array (or a
//! DataView, a Node Buffer) only throws: views carry [[ViewedArrayBuffer]],
//! never [[ArrayBufferData]] / [[ArrayBufferMaxByteLength]] (10.4.5,
//! 23.2.5.1.1 AllocateTypedArray, 25.3.2.1; RequireInternalSlot in
//! 25.1.6.6 step 2, 25.1.6.7 step 2, 25.1.6.8-9).
use super::*;

const OBJ: &str = "const o = { m() { return 1; } };\n";

fn m_calls(src: &str) -> Vec<String> {
    let path = "t.ts".to_string();
    let module = perry_parser::parse_typescript(src, &path).expect("parses");
    let hir = perry_hir::lower_module(&module, "t", &path).expect("lowers");
    let pf = crate::compute(&[(path, &hir)], &FactsConfig::default());
    let mut s: Vec<_> = pf.sites.iter().filter(|s| s.key == "m" && s.is_call).collect();
    s.sort_by_key(|s| s.byte_offset);
    let v: Vec<String> = s.iter().map(|s| format!("{:?}", s.verdict)).collect();
    eprintln!("SKINROOT14 {src:?}: {v:?}");
    assert!(!v.is_empty(), "no call site of m");
    v
}

fn mono(v: &[String]) -> bool {
    v.iter().all(|x| x == "T0Mono" || x.starts_with("T0Mono("))
}

/// Proven: one callee (`T0Mono`) or a proven set of callees (`T0Set`).
fn proven(v: &[String]) -> bool {
    v.iter().all(|x| x.starts_with("T0Mono") || x.starts_with("T0Set"))
}

/// A second object whose `m` the program-written function puts on `t`.
const OBJ2: &str = "const o2 = { m() { return 2; } };\n";

/// (b) the skin shape: a view whose field holds the method, called as its
/// method; `o` is another field of it.
fn view_call(view: &str, f: &str) -> String {
    format!("{OBJ}const v: any = {view};\nv.k = o;\nv.r = {f};\ntry {{ v.r(1); }} catch (e) {{}}\nconst r = v.k.m();\n")
}

#[test]
fn red_skinroot14_buffer_method_on_typed_array_only_throws() {
    for f in [
        "ArrayBuffer.prototype.resize",
        "ArrayBuffer.prototype.slice",
        "ArrayBuffer.prototype.transfer",
        "ArrayBuffer.prototype.transferToFixedLength",
        "SharedArrayBuffer.prototype.grow",
    ] {
        let v = m_calls(&view_call("new Int8Array(4)", f));
        assert!(proven(&v), "{f} on a typed array only throws: v does not escape: {v:?}");
    }
}

/// OPEN (SKINROOT15): with the brand rule the call only throws, but `v.k`
/// on a DataView also holds every primitive ("primitive beside an object
/// (key not absent on its family)"), which an ArrayBuffer subject does not
/// show: a second gap of the DataView `k` read, not this rule.
#[test]
#[ignore = "open: DataView named read of a program key adds every primitive"]
fn red_skinroot14_buffer_method_on_data_view_only_throws() {
    let v = m_calls(&view_call("new DataView(new ArrayBuffer(8))", "ArrayBuffer.prototype.resize"));
    assert!(mono(&v), "resize on a DataView only throws: {v:?}");
}

/// Soundness: the receiver may be an instance of a class extending
/// ArrayBuffer (its `super()` allocates [[ArrayBufferData]]): the call keeps
/// the method's path; `o` is not claimed.
#[test]
fn keep_skinroot14_maybe_buffer_subclass_receiver() {
    let src = format!("{OBJ}class B extends ArrayBuffer {{ k: any = o; }}\nconst v: any = Math.random() > 0.5 ? new B(8) : new Int8Array(4);\nv.k = o;\nv.r = ArrayBuffer.prototype.resize;\ntry {{ v.r(1); }} catch (e) {{}}\nconst r = v.k.m();\n");
    let v = m_calls(&src);
    assert!(!mono(&v), "an ArrayBuffer subclass instance may carry the slot: {v:?}");
}

/// (a) the GLTFLoader shape: what a same-object for-in read finds on a
/// buffer is stored on an object and called as its method.
fn forin(subject: &str, pre: &str) -> String {
    format!("{OBJ}const b: any = {subject};\n{pre}const t: any = {{ k: o }};\nfor (const name in b) {{ t[name] = b[name]; }}\ntry {{ t.push(1); }} catch (e) {{}}\ntry {{ t.slice(1); }} catch (e) {{}}\nconst r = t.k.m();\n")
}

#[test]
fn red_skinroot14_forin_over_buffer_reads_no_prototype_member() {
    // DataView: the open gap above (every primitive beside `o`).
    for s in ["new ArrayBuffer(8)", "new SharedArrayBuffer(8)"] {
        let v = m_calls(&forin(s, ""));
        assert!(mono(&v), "for-in over {s} reads no built-in member: {v:?}");
    }
}

/// The read finds what the program wrote on the buffer (an own expando):
/// the function stored under `push` is read, stored on `t` and called with
/// `t` as `this`; it puts `o2` in `t.k`, so `t.k.m()` has two callees. The
/// verdict is `T0Mono` (`o.m` alone) only if the read dropped the member.
#[test]
fn keep_skinroot14_forin_over_buffer_reads_program_member() {
    // Not a DataView: its open gap (every primitive) is not `T0Mono` either,
    // so its assertion could not fail.
    for s in ["new ArrayBuffer(8)", "new SharedArrayBuffer(8)"] {
        let v = m_calls(&format!(
            "{OBJ2}{}",
            forin(s, "b.push = function (this: any) { this.k = o2; };\n")
        ));
        assert!(!mono(&v), "the program-written own member of {s} is read and runs on t: {v:?}");
    }
}

/// A program-written member of `ArrayBuffer.prototype` or
/// `Object.prototype` is enumerable (an assignment creates it so, 10.1.9
/// OrdinarySet -> CreateDataProperty): for-in visits it (14.7.5.9) and the
/// read finds it. A member written onto a built-in prototype is stored on
/// `t` under every name (the key is not one name), `k` among them: the
/// function itself is a receiver of `t.k.m()`, the verdict "receiver is a
/// builtin/function value". A read that dropped the member leaves no
/// function in `t.k` (SKINROOT15 sabotage: "escape: stored into a value
/// from unmodeled code").
#[test]
fn keep_skinroot14_forin_over_buffer_reads_program_prototype_member() {
    for p in ["ArrayBuffer.prototype", "Object.prototype"] {
        let pre = format!("({p} as any).push = function (this: any) {{ this.k = o2; }};\n");
        let v = m_calls(&format!("{OBJ2}{}", forin("new ArrayBuffer(8)", &pre)));
        assert!(
            v.iter().any(|x| x.contains("receiver is a builtin/function value")),
            "the program-written member of {p} is read: the function reaches t.k: {v:?}"
        );
    }
}
