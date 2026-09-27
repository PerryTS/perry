//! #11590: the packed-f64 range loop's cached copy of a module global must be
//! a GC root when the global can hold a heap value.
//!
//! `lower_packed_f64_range_versioned_for` copies every loop-invariant module
//! global the body reads into an entry alloca and aliases it into `ctx.locals`
//! for BOTH loop clones. For
//!
//! ```ts
//! let d: any = [];                       // read by name in some function
//! for (let j = 0; j < 80; j++) d[j] = v; // module scope
//! ```
//!
//! the entry guard fails (length 0), the SLOW clone runs, and it polls for GC
//! on its back-edge and grows `d` through `js_dyn_index_set_strict` every
//! iteration — all through that cache. An evacuating minor rewrites
//! `@perry_global_*` (a registered root) but not a bare alloca, so the next
//! store dereferenced from-space: SIGSEGV under `PERRY_GC_SCHEDULE_SEED` +
//! `PERRY_GC_PROTECT_FROMSPACE=1` (the #10514 `grown`/`grown_typed` benches).
//!
//! The assertions read `main()`'s IR under both root lowerings. Each names the
//! exact slot the global's value is first stored into, so a root slot reserved
//! for something else in `main()` cannot satisfy them.
//!
//! Sabotage: force `may_hold_pointer` to `false` in `stmt/loops.rs` and
//! `a_heap_valued_global_cache_is_rooted_*` go red.

use crate::codegen::helpers::NativeRootsPin;
use perry_hir::types::Type;
use perry_hir::{BinaryOp, CompareOp, Expr, Function, Module, ModuleInitKind, Stmt, UpdateOp};

const D: u32 = 0;
const J: u32 = 1;

fn counted_store_loop(target: u32, value: Expr) -> Stmt {
    Stmt::For {
        init: Some(Box::new(Stmt::Let {
            id: J,
            name: "j".to_string(),
            ty: Type::Number,
            mutable: true,
            init: Some(Expr::Integer(0)),
        })),
        condition: Some(Expr::Compare {
            op: CompareOp::Lt,
            left: Box::new(Expr::LocalGet(J)),
            right: Box::new(Expr::Integer(80)),
        }),
        update: Some(Expr::Update {
            id: J,
            op: UpdateOp::Increment,
            prefix: false,
        }),
        body: vec![Stmt::Expr(Expr::PutValueSet {
            target: Box::new(Expr::LocalGet(target)),
            key: Box::new(Expr::LocalGet(J)),
            value: Box::new(value),
            receiver: Box::new(Expr::LocalGet(target)),
            strict: false,
        })],
    }
}

/// `let d: any = []; for (…) d[j] = j * 7; function read() { return d }`.
fn grown_global_module() -> Module {
    let mut m = Module::new("grown_global.ts");
    m.functions.push(Function {
        id: 0,
        name: "read".to_string(),
        type_params: Vec::new(),
        params: Vec::new(),
        return_type: Type::Any,
        body: vec![Stmt::Return(Some(Expr::LocalGet(D)))],
        is_async: false,
        is_generator: false,
        is_strict: true,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    });
    m.init = vec![
        Stmt::Let {
            id: D,
            name: "d".to_string(),
            ty: Type::Any,
            mutable: true,
            init: Some(Expr::Array(Vec::new())),
        },
        counted_store_loop(
            D,
            Expr::Binary {
                op: BinaryOp::Mul,
                left: Box::new(Expr::LocalGet(J)),
                right: Box::new(Expr::Integer(7)),
            },
        ),
    ];
    m.init_kind = ModuleInitKind::Eager;
    m
}

fn main_ir(m: &Module) -> String {
    let opts = crate::CompileOptions {
        emit_ir_only: true,
        is_entry_module: true,
        ..Default::default()
    };
    let ir = String::from_utf8(crate::compile_module(m, opts).expect("module compiles"))
        .expect("LLVM IR is UTF-8");
    let start = ir
        .find("define i32 @main()")
        .expect("entry module emits main()");
    let rest = &ir[start..];
    let end = rest.find("\n}\n").unwrap_or(rest.len());
    rest[..end].to_string()
}

const GLOBAL: &str = "@perry_global_grown_global_ts__0";

/// The cache is the slot the global's value is stored into right after the
/// guard-preamble load.
fn cache_slot(main: &str) -> String {
    let lines: Vec<&str> = main.lines().map(str::trim).collect();
    // The packed tier is the subject; without its guard nothing below means
    // anything (CLAUDE.md: a gate must assert its subject was live).
    assert!(
        main.contains("packed_f64_range"),
        "premise: the loop must lower through the packed-f64 range tier\n{main}"
    );
    let load_reg = lines
        .iter()
        .rev()
        .filter_map(|l| {
            let (lhs, rhs) = l.split_once(" = ")?;
            (rhs == format!("load double, ptr {GLOBAL}")).then(|| lhs.to_string())
        })
        .next()
        .unwrap_or_else(|| panic!("premise: main() must load {GLOBAL}\n{main}"));
    // Native lowering stores `inttoptr(bitcast %load)`; shadow stores the
    // double itself. Follow the value through those two casts.
    let mut names = vec![load_reg];
    for l in &lines {
        if let Some((lhs, rhs)) = l.split_once(" = ") {
            if (rhs.starts_with("bitcast double ") || rhs.starts_with("inttoptr i64 "))
                && names.iter().any(|n| rhs.contains(&format!(" {n} ")))
            {
                names.push(lhs.to_string());
            }
        }
    }
    lines
        .iter()
        .find_map(|l| {
            let rest = l.strip_prefix("store ")?;
            let (val, slot) = rest.rsplit_once(", ptr ")?;
            let val_reg = val.rsplit(' ').next()?;
            (names.iter().any(|n| n == val_reg) && slot.starts_with('%')).then(|| slot.to_string())
        })
        .unwrap_or_else(|| panic!("premise: the loaded global must be cached in a slot\n{main}"))
}

#[test]
fn a_heap_valued_global_cache_is_rooted_native() {
    let _pin = NativeRootsPin::native();
    let main = main_ir(&grown_global_module());
    let slot = cache_slot(&main);
    assert!(
        main.contains(&format!("{slot} = alloca ptr addrspace(1)")),
        "#11590: the packed loop's cache of heap-valued global {GLOBAL} ({slot}) must be \
         a native root (`alloca ptr addrspace(1)`), or an evacuating minor leaves it \
         pointing into from-space\n{main}"
    );
}

#[test]
fn a_heap_valued_global_cache_is_rooted_shadow() {
    let _pin = NativeRootsPin::shadow();
    let main = main_ir(&grown_global_module());
    let slot = cache_slot(&main);
    assert!(
        main.lines()
            .any(|l| l.contains("@js_shadow_slot_bind(") && l.contains(&format!("ptr {slot})"))),
        "#11590: the packed loop's cache of heap-valued global {GLOBAL} ({slot}) must be \
         bound as a shadow root\n{main}"
    );
}
