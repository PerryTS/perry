#![no_std]
//! Runtime layout facts that generated code bakes in, in ONE file both sides
//! depend on: `perry-runtime` (as `crate::codegen_abi`) and `perry-codegen`
//! (as `crate::runtime_abi`). A layout change edits a number here, and the
//! runtime's `offset_of!`/`size_of` assertions next to each struct refuse to
//! compile until the number is right — so emitted code can never disagree
//! with the struct it indexes. No dependencies.

/// `array::ArrayHeader` size: element 0 follows it.
pub const ARRAY_HEADER_SIZE: usize = 8;

/// `agent_ptrs::PERRY_AGENT_PTRS`: the number of per-agent pointer slots.
/// Slot 0 is reserved (the megamorphic follow-up's shape-record directory).
pub const AGENT_PTR_SLOTS: usize = 4;
/// Slot 1: the address of this agent's implicit-`this` cell
/// (`tls_hot::HotTls::implicit_this`), which a direct method call binds.
pub const AGENT_PTR_IMPLICIT_THIS: usize = 1;
/// `tls_hot::HotTls::agent_ptrs` (Apple aarch64 TSD path; LP64): directly
/// after `implicit_this` (128), behind fixed-size fields only.
pub const HOT_TLS_AGENT_PTRS_OFFSET: usize = 136;

/// `closure::ClosureHeader` (LP64): the u32 capture count at 0, the ShapeId
/// at 4 (the same word as `ObjectHeader`), the code pointer at 8, the shaped
/// own-property record at 16, captures from 24. ILP32 targets shrink the two
/// pointers: code pointer at 8, props at 12, captures from 16 (derived in
/// `perry-codegen/src/target_layout.rs`).
pub const CLOSURE_SHAPE_OFFSET: usize = 4;
pub const CLOSURE_FUNC_PTR_OFFSET: usize = 8;
pub const CLOSURE_PROPS_OFFSET: usize = 16;
pub const CLOSURE_HEADER_SIZE: usize = 24;

/// `gc::GC_TYPE_CLOSURE`: the GcHeader type byte (at payload - 8) that makes a
/// cell a function object. The kind is this byte, never a payload magic.
pub const GC_TYPE_CLOSURE: u8 = 4;
/// `gc::GC_FLAG_FORWARDED` (GcHeader byte 1): an evacuated from-space stub.
pub const GC_FLAG_FORWARDED: u8 = 0x80;
/// `gc::GC_HEADER_SIZE`.
pub const GC_HEADER_SIZE: usize = 8;

/// The JS BODY calling convention. Every native body a function object runs —
/// a compiled closure body, a value wrapper, a native builtin installed as a
/// function object — is
///
/// ```text
/// double body(i64 callee, i64 this, double a0, double a1, ...)
/// ```
///
/// where `callee` is the function object (its captures follow the header) and
/// `this` is the NaN-boxed receiver bits, passed in an INTEGER register so
/// every floating-point argument register stays free for JS arguments (SysV
/// x86-64 / AAPCS64; Win64 assigns positionally, which is equally correct).
/// Passing more JS arguments than a body declares is safe (the caller owns
/// the stack argument area); fewer is padded with `undefined` by the caller.
/// The runtime calls bodies only through `closure/body_call.rs`; emitted code
/// only through `expr::body_call::emit_js_body_call`.
///
/// While the implicit-`this` cell still exists (this-as-a-parameter stage 1),
/// a caller passes exactly the receiver the cell holds for the call, so the
/// parameter and the cell never disagree; bodies still read the cell.
pub const JS_BODY_CALLEE_PARAM: usize = 0;
/// Native parameter index of the receiver (`this`) bits.
pub const JS_BODY_THIS_PARAM: usize = 1;
/// Native parameter index of the first JS argument.
pub const JS_BODY_FIRST_ARG_PARAM: usize = 2;
/// Native parameters every JS body declares before its JS arguments.
pub const JS_BODY_FIXED_PARAMS: usize = 2;
/// `js_closure_call{N}(callee, a0..aN-1)` exists for `N <= JS_CLOSURE_CALL_MAX_ARGS`;
/// wider calls use `js_closure_call_array`.
pub const JS_CLOSURE_CALL_MAX_ARGS: usize = 16;
/// The fixed-arity entries, indexed by JS argument count.
pub const JS_CLOSURE_CALL_ENTRIES: [&str; JS_CLOSURE_CALL_MAX_ARGS + 1] = [
    "js_closure_call0",
    "js_closure_call1",
    "js_closure_call2",
    "js_closure_call3",
    "js_closure_call4",
    "js_closure_call5",
    "js_closure_call6",
    "js_closure_call7",
    "js_closure_call8",
    "js_closure_call9",
    "js_closure_call10",
    "js_closure_call11",
    "js_closure_call12",
    "js_closure_call13",
    "js_closure_call14",
    "js_closure_call15",
    "js_closure_call16",
];
/// Every runtime entry point emitted code calls to run a JS function. Each can
/// run arbitrary JS and therefore collect: `scripts/gc_root_dominance_check.py`
/// reads its poll-capable set from THIS list.
pub const JS_CALL_ENTRIES: [&str; JS_CLOSURE_CALL_MAX_ARGS + 5] = [
    "js_closure_call0",
    "js_closure_call1",
    "js_closure_call2",
    "js_closure_call3",
    "js_closure_call4",
    "js_closure_call5",
    "js_closure_call6",
    "js_closure_call7",
    "js_closure_call8",
    "js_closure_call9",
    "js_closure_call10",
    "js_closure_call11",
    "js_closure_call12",
    "js_closure_call13",
    "js_closure_call14",
    "js_closure_call15",
    "js_closure_call16",
    "js_closure_call1_receiverless",
    "js_closure_call_array",
    "js_closure_call_apply_with_spread",
    "js_native_call_value",
];
/// `object::method_site::MethodEntry` — the words the emitted method-call site
/// reads (`perry-codegen/src/expr/method_site.rs`).
pub const METHOD_SITE_WORD_OFFSET: usize = 0;
pub const METHOD_SITE_SLOT_OFFSET: usize = 8;
pub const METHOD_SITE_FUNC_OFFSET: usize = 16;
pub const METHOD_SITE_CLOSURE_OFFSET: usize = 24;
pub const METHOD_SITE_GEN_OFFSET: usize = 32;
/// Entries per method site, and one entry's size.
pub const METHOD_SITE_WAYS: usize = 2;
pub const METHOD_SITE_ENTRY_SIZE: usize = 40;
/// A method site calls a body with its argument count padded by `undefined`
/// up to this many extra arguments (never past 16), and admits bodies that
/// declare up to that many parameters.
pub const METHOD_SITE_ARG_PAD: usize = 3;
pub const fn method_site_padded_argc(argc: usize) -> usize {
    let padded = argc + METHOD_SITE_ARG_PAD;
    if padded > 16 {
        if argc > 16 {
            argc
        } else {
            16
        }
    } else {
        padded
    }
}
/// The entry `slot` bit for an own key in the receiver's spill buffer.
pub const METHOD_SITE_SPILL: u64 = 1 << 62;
/// The entry `slot` bit for an own key of a function-object receiver: an
/// inline slot of the object at `ClosureHeader::props`.
pub const METHOD_SITE_FUNCTION_BAG: u64 = 1 << 61;
/// The index bits of an entry's `slot` word (bit 60 is reserved for the
/// accessor entry kind).
pub const METHOD_SITE_INDEX_MASK: u64 = (1 << 60) - 1;
/// `object::ObjectMeta::spill` (the object-owned overflow buffer).
pub const OBJECT_META_SPILL_OFFSET: usize = 32;
