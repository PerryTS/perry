//! The dense prefix P of the element-kind word (EK design §3-§4) on emitted
//! IR: births set it, inline frontier stores raise it, and the guarded read
//! serves `index < P` without the bounds and hole tests. Each test fails
//! when its mechanism is removed.
use perry_hir::types::Type;
use perry_hir::{Expr, Function, Module as HirModule, Param, Stmt};

fn module_ir(params: Vec<Param>, body: Vec<Stmt>) -> String {
    let mut hir = HirModule::new("ek_prefix_test");
    hir.functions.push(Function {
        id: 1,
        name: "probe".to_string(),
        type_params: Vec::new(),
        params,
        return_type: Type::Any,
        body,
        is_async: false,
        is_generator: false,
        is_strict: true,
        is_exported: true,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    });
    let opts = crate::CompileOptions {
        emit_ir_only: true,
        ..Default::default()
    };
    let bytes = crate::compile_module(&hir, opts).expect("test module compiles");
    let ll = String::from_utf8(bytes).expect("LLVM IR is UTF-8");
    #[cfg(feature = "llvm-inprocess")]
    crate::testing::verify_ir(&ll, "ek_prefix_test").unwrap();
    ll
}

fn param(id: u32, name: &str, ty: Type) -> Param {
    Param {
        id,
        name: name.into(),
        ty,
        default: None,
        decorators: vec![],
        is_rest: false,
        arguments_object: None,
    }
}

fn local_array(init: Expr) -> Stmt {
    Stmt::Let {
        id: 0,
        name: "a".to_string(),
        ty: Type::Array(Box::new(Type::Any)),
        mutable: true,
        init: Some(init),
    }
}

/// The instructions of the first block whose label contains `tag`.
fn block<'a>(ll: &'a str, tag: &str) -> Vec<&'a str> {
    let mut out = Vec::new();
    let mut inside = false;
    for line in ll.lines() {
        let is_label = !line.starts_with(' ') && line.trim_end().ends_with(':');
        if is_label {
            if inside {
                break;
            }
            inside = line.contains(tag);
            continue;
        }
        if inside {
            out.push(line);
        }
    }
    assert!(!out.is_empty(), "no block labelled *{tag}* in the IR");
    out
}

fn dest(line: &str) -> Option<&str> {
    let line = line.trim();
    line.split_once(" = ").map(|(d, _)| d)
}

/// The pointers the IR forms as `inttoptr (add i64 %h, 16)`: the address of
/// P on an array payload.
fn prefix_pointers(ll: &str) -> Vec<String> {
    let offset = format!(", {}", crate::target_layout::ARRAY_EK_PREFIX_OFFSET_BYTES);
    let adds: Vec<&str> = ll
        .lines()
        .filter(|l| l.contains("= add i64 ") && l.trim_end().ends_with(&offset))
        .filter_map(dest)
        .collect();
    ll.lines()
        .filter(|l| l.contains("= inttoptr i64 "))
        .filter(|l| {
            adds.iter()
                .any(|a| l.contains(&format!("inttoptr i64 {a} to")))
        })
        .filter_map(dest)
        .map(str::to_owned)
        .collect()
}

/// `store i32 <v>, ptr <P address>` instructions.
fn prefix_stores(ll: &str) -> Vec<String> {
    let ptrs = prefix_pointers(ll);
    ll.lines()
        .filter(|l| l.trim_start().starts_with("store i32 "))
        .filter(|l| {
            ptrs.iter().any(|p| {
                l.trim_end().ends_with(&format!("ptr {p}")) || l.contains(&format!("ptr {p},"))
            })
        })
        .map(|l| l.trim().to_owned())
        .collect()
}

fn def<'a>(ll: &'a str, reg: &str) -> Option<&'a str> {
    ll.lines().find(|l| dest(l) == Some(reg)).map(str::trim)
}

fn regs(line: &str) -> Vec<&str> {
    let rhs = line.split_once(" = ").map_or(line, |(_, r)| r);
    rhs.split(|c: char| c == ' ' || c == ',')
        .filter(|t| t.starts_with('%'))
        .collect()
}

/// The label of the block holding `line`.
fn block_of<'a>(ll: &'a str, line: &str) -> Option<&'a str> {
    let mut label = None;
    for l in ll.lines() {
        let first = l.split_whitespace().next().unwrap_or("");
        if !l.starts_with(char::is_whitespace) && first.ends_with(':') {
            label = Some(first.trim_end_matches(':'));
        }
        if l.trim() == line {
            return label;
        }
    }
    None
}

/// Stores of exactly `P + zext(bits != TAG_HOLE)` into P in a block entered
/// only on the taken edge of `br i1 (index == P)` (the frontier row, with the
/// raise branched so a store below the frontier skips the read-modify-write
/// of the word): a raise without the `index == P` branch or without the hole
/// test does not count.
fn frontier_raises(ll: &str) -> usize {
    let ptrs = prefix_pointers(ll);
    let is_prefix_load = |reg: &str| {
        def(ll, reg).is_some_and(|d| {
            d.contains("load i32, ptr ")
                && ptrs
                    .iter()
                    .any(|p| d.ends_with(&format!("ptr {p}")) || d.contains(&format!("ptr {p},")))
        })
    };
    let mut n = 0;
    for store in prefix_stores(ll) {
        let Some(value) = regs(&store).first().map(|r| r.to_string()) else {
            continue;
        };
        let Some(add) = def(ll, &value).filter(|d| d.contains("add i32 ")) else {
            continue;
        };
        let ops = regs(add);
        let Some(load) = ops.iter().find(|r| is_prefix_load(r)) else {
            continue;
        };
        let Some(z) = ops.iter().find(|r| *r != load) else {
            continue;
        };
        let Some(zext) = def(ll, z).filter(|d| d.contains("zext i1 ")) else {
            continue;
        };
        let not_hole = regs(zext)
            .first()
            .and_then(|r| def(ll, r))
            .is_some_and(|d| d.contains("icmp ne i64 ") && d.contains(crate::nanbox::TAG_HOLE_I64));
        let Some(raise_block) = block_of(ll, &store) else {
            continue;
        };
        let taken = format!(", label %{raise_block}, label %");
        let at_frontier = ll.lines().map(str::trim).any(|l| {
            l.starts_with("br i1 ")
                && l.contains(&taken)
                && regs(l)
                    .first()
                    .and_then(|c| def(ll, c))
                    .is_some_and(|d| d.contains("icmp eq i32 ") && regs(d).contains(load))
        });
        if at_frontier && not_hole {
            n += 1;
        }
    }
    n
}

fn read_ir(numeric: bool) -> String {
    let elem = if numeric { Type::Number } else { Type::Any };
    module_ir(
        vec![
            param(1, "array", Type::Array(Box::new(elem))),
            param(2, "key", Type::Any),
        ],
        vec![Stmt::Return(Some(Expr::IndexGet {
            byte_offset: 0,
            byte_end: 0,
            object: Box::new(Expr::LocalGet(1)),
            index: Box::new(Expr::Integer(1)),
        }))],
    )
}

/// EK §4: the guard tests `index <u P` right after the type word, and the
/// hit loads the element with no length/capacity compare and no hole test.
#[test]
fn prefix_hit_reads_without_bounds_or_hole_tests() {
    let ll = read_ir(false);
    let guard = block(&ll, "guard.prefix");
    assert!(
        guard.iter().any(|l| l.contains("icmp ult i32")),
        "the prefix test is a strict unsigned `index < P`: {guard:#?}"
    );
    assert!(
        !guard
            .iter()
            .any(|l| l.contains("icmp ule") || l.contains("icmp sl")),
        "never `<=` or a signed compare: {guard:#?}"
    );
    assert!(
        !guard.iter().any(|l| l.contains("perry_array_element_kind")),
        "the descriptor query stays off the prefix test"
    );
    let hit = block(&ll, "prefix.hit");
    assert!(hit.iter().any(|l| l.contains("load i64")), "{hit:#?}");
    assert!(
        !hit.iter()
            .any(|l| l.contains("icmp") || l.contains(crate::nanbox::TAG_HOLE_I64)),
        "a prefix hit needs no compare and no hole test: {hit:#?}"
    );
    // The miss keeps the full guard: the hole test is still in the IR.
    assert!(
        ll.contains(crate::nanbox::TAG_HOLE_I64),
        "the miss path keeps its hole test"
    );
}

/// The numeric tier keeps its is_number test on a prefix hit: P proves a
/// non-hole element, never a Number (types pick the lowering, the element
/// proves the semantics).
#[test]
fn numeric_prefix_hit_keeps_its_number_test() {
    let ll = read_ir(true);
    let hit = block(&ll, "prefix.hit");
    assert!(hit.iter().any(|l| l.contains("load i64")), "{hit:#?}");
    assert!(
        hit.iter().any(|l| l.contains("icmp")) && hit.iter().any(|l| l.contains("br i1")),
        "the numeric hit tests the loaded element: {hit:#?}"
    );
}

/// EK §3.1 birth row: an inline literal of n elements is born with P = n,
/// written into the element-kind word at payload +16 (raw +24).
#[test]
fn inline_literal_is_born_with_its_prefix() {
    let ll = module_ir(
        vec![],
        vec![
            local_array(Expr::Array(
                vec![Expr::Number(1.0), Expr::Number(2.0), Expr::Number(3.0)],
                None,
            )),
            Stmt::Return(Some(Expr::LocalGet(0))),
        ],
    );
    let word = 8 + crate::target_layout::ARRAY_EK_PREFIX_OFFSET_BYTES;
    let geps: Vec<&str> = ll
        .lines()
        .filter(|l| {
            l.contains("getelementptr i8") && l.trim_end().ends_with(&format!("i64 {word}"))
        })
        .filter_map(dest)
        .collect();
    assert!(
        ll.lines()
            .any(|l| l.trim_start().starts_with("store i64 3, ptr")
                && geps
                    .iter()
                    .any(|g| l.trim_end().ends_with(&format!("ptr {g}")))),
        "the literal stores P = 3 into its element-kind word"
    );
}

/// The outlined literal (more than the inline maximum) stores P = n after
/// its slot fill.
#[test]
fn outlined_literal_is_born_with_its_prefix() {
    let n = 40usize;
    let ll = module_ir(
        vec![],
        vec![
            local_array(Expr::Array(
                (0..n).map(|i| Expr::Number(i as f64 + 0.5)).collect(),
                None,
            )),
            Stmt::Return(Some(Expr::LocalGet(0))),
        ],
    );
    let offset = crate::target_layout::ARRAY_EK_PREFIX_OFFSET_BYTES;
    let geps: Vec<&str> = ll
        .lines()
        .filter(|l| {
            l.contains("getelementptr i8") && l.trim_end().ends_with(&format!("i64 {offset}"))
        })
        .filter_map(dest)
        .collect();
    assert!(
        ll.lines().any(
            |l| l.trim_start().starts_with(&format!("store i32 {n}, ptr"))
                && geps
                    .iter()
                    .any(|g| l.trim_end().ends_with(&format!("ptr {g}")))
        ),
        "the outlined literal stores P = {n}"
    );
}

fn store_ir(stmt: Stmt) -> String {
    module_ir(
        vec![param(1, "v", Type::Any)],
        vec![
            local_array(Expr::Array(Vec::new(), None)),
            stmt,
            Stmt::Return(Some(Expr::LocalGet(0))),
        ],
    )
}

/// EK §3.1 frontier row on every inline writer: a store at `index == P`
/// raises P (`P + zext(value != hole)` behind a branch on `index == P`).
#[test]
fn inline_writers_raise_the_frontier() {
    let push = store_ir(Stmt::Expr(Expr::ArrayPush {
        array_id: 0,
        value: Box::new(Expr::LocalGet(1)),
        field_writeback: None,
    }));
    assert!(
        frontier_raises(&push) >= 1,
        "push raises P: {:#?}",
        prefix_stores(&push)
    );
    let append = store_ir(Stmt::Expr(Expr::IndexSet {
        object: Box::new(Expr::LocalGet(0)),
        index: Box::new(Expr::PropertyGet {
            byte_end: 0,
            object: Box::new(Expr::LocalGet(0)),
            property: "length".into(),
            byte_offset: 0,
        }),
        value: Box::new(Expr::LocalGet(1)),
    }));
    assert!(frontier_raises(&append) >= 1, "a[a.length] = v raises P");
    let indexed = store_ir(Stmt::Expr(Expr::IndexSet {
        object: Box::new(Expr::LocalGet(0)),
        index: Box::new(Expr::Integer(0)),
        value: Box::new(Expr::LocalGet(1)),
    }));
    assert!(
        frontier_raises(&indexed) >= 2,
        "a[i] = v raises P on its in-bounds and its extend arm: {:#?}",
        prefix_stores(&indexed)
    );
    // A property receiver takes `emit_guarded_inbounds_array_store`.
    let guarded = super::index_set_barrier_tests::ir();
    assert!(frontier_raises(&guarded) >= 1, "this.vals[i] = v raises P");
}

/// The dynamic `obj[i]` IC (`inline_dyn_typed_array.rs`): its ordinary-Array
/// arm tests `index < P` first, and the hit loads with no descriptor,
/// prototype or bounds guard and no hole select.
#[test]
fn dynamic_read_ic_takes_the_prefix_hit_first() {
    let ll = module_ir(
        vec![param(1, "o", Type::Any), param(2, "i", Type::Number)],
        vec![Stmt::Return(Some(Expr::IndexGet {
            byte_offset: 0,
            byte_end: 0,
            object: Box::new(Expr::LocalGet(1)),
            index: Box::new(Expr::LocalGet(2)),
        }))],
    );
    let guard = block(&ll, "arrlike.ic.array_guard");
    assert!(
        guard.iter().any(|l| l.contains("icmp ult i64")),
        "the array arm opens with `index <u P`: {guard:#?}"
    );
    assert!(
        !guard.iter().any(|l| l.contains("perry_array_element_kind")
            || l.contains("PERRY_ARRAY_INDEX_FAST_PATH_INVALIDATED")),
        "the descriptor and prototype guard stay on the miss: {guard:#?}"
    );
    let hit = block(&ll, "arrlike.ic.prefix_hit");
    assert!(hit.iter().any(|l| l.contains("load double")), "{hit:#?}");
    assert!(
        !hit.iter()
            .any(|l| l.contains("icmp") || l.contains("select")),
        "a prefix hit has no guard and no hole select: {hit:#?}"
    );
}
