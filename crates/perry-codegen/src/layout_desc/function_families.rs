//! Ordinary function constructor layouts. Immutable info tails name existing
//! rooted bindings and current closure captures; admission never evaluates JS.
use super::{section, ATOM_TY, DESC_TY, KEY_TY};
use crate::{module::LlModule, strings::StringPool};
use perry_hir::{Expr, Module, Stmt};
use std::collections::{BTreeMap, HashMap, HashSet};

#[derive(Clone, Debug, PartialEq, Eq)]
enum Key {
    String(String),
    Binding(u32),
}
#[derive(Clone, Debug)]
struct Operand {
    global: Option<String>,
    capture: u32,
    kind: u16,
    slot: u16,
}
#[derive(Clone, Debug)]
struct Plan {
    body: String,
    keys: Vec<Key>,
    operands: HashMap<u32, Operand>,
    /// The module constructs the function by name ([`constructed`]): its
    /// stores take the run ([`run_arm`]).
    constructed: bool,
}

fn key(expr: &Expr, has_operand: &impl Fn(u32) -> bool) -> Option<Key> {
    match expr {
        Expr::String(k) => Some(Key::String(k.clone())),
        Expr::LocalGet(id) if has_operand(*id) => Some(Key::Binding(*id)),
        _ => None,
    }
}
/// `expr` as a store into `this`: its key (`None` when the prefix cannot
/// name it) and its value.
fn this_store<'e>(
    expr: &'e Expr,
    has_operand: &impl Fn(u32) -> bool,
) -> Option<(Option<Key>, &'e Expr)> {
    match expr {
        Expr::PropertySet {
            object,
            property,
            value,
            ..
        } if matches!(object.as_ref(), Expr::This) => {
            Some((Some(Key::String(property.clone())), value))
        }
        Expr::IndexSet {
            object,
            index,
            value,
            ..
        } if matches!(object.as_ref(), Expr::This) => Some((key(index, has_operand), value)),
        Expr::PutValueSet {
            target,
            receiver,
            key: k,
            value,
            ..
        } if matches!(target.as_ref(), Expr::This) && matches!(receiver.as_ref(), Expr::This) => {
            Some((key(k, has_operand), value))
        }
        _ => None,
    }
}
fn prefix(body: &[Stmt], has_operand: &impl Fn(u32) -> bool) -> Vec<Key> {
    /// Append the keys the store `expr` adds, in the order it adds them. The
    /// value is evaluated before the store, so a chained `this.a = this.b =
    /// v` adds `b` first: a layout listing `a` first would send every birth
    /// to the overflow as an out-of-order add. `false` ends the prefix.
    fn store(expr: &Expr, has_operand: &impl Fn(u32) -> bool, keys: &mut Vec<Key>) -> bool {
        let Some((k, value)) = this_store(expr, has_operand) else {
            return false;
        };
        if this_store(value, has_operand).is_some() && !store(value, has_operand, keys) {
            return false;
        }
        let Some(k) = k else {
            return false;
        };
        if matches!(&k, Key::String(s) if s == "__proto__" || !super::key_is_atom(s)) {
            return false;
        }
        if !keys.contains(&k) {
            keys.push(k);
        }
        keys.len() < 64
    }
    fn scan(body: &[Stmt], has_operand: &impl Fn(u32) -> bool, keys: &mut Vec<Key>) -> bool {
        for stmt in body {
            match stmt {
                Stmt::PreallocateBoxes(_) | Stmt::PreallocateTdzBoxes(_) | Stmt::Let { .. } => {
                    continue
                }
                Stmt::Expr(e) => {
                    if !store(e, has_operand, keys) {
                        return false;
                    }
                }
                _ => return false,
            }
        }
        true
    }
    let mut keys = Vec::new();
    scan(body, has_operand, &mut keys);
    keys
}
thread_local! { static PLANS: std::cell::RefCell<Vec<Plan>> = const { std::cell::RefCell::new(Vec::new()) }; }

/// The module's constructor-layout plans as a parallel lowering worker must
/// see them (`codegen::callable_workers`): a store site reads them while its
/// body is lowered, so a worker without them would lower the generic store.
#[derive(Clone)]
pub(crate) struct CallableState(Vec<Plan>);

pub(crate) fn callable_state() -> CallableState {
    CallableState(PLANS.with(|p| p.borrow().clone()))
}

impl CallableState {
    pub(crate) fn install(&self) {
        PLANS.with(|p| *p.borrow_mut() = self.0.clone());
    }
}

fn plan(
    body: String,
    stmts: &[Stmt],
    constructed: bool,
    operand: impl Fn(u32) -> Option<Operand>,
) -> Option<Plan> {
    let keys = prefix(stmts, &|id| operand(id).is_some());
    if keys.is_empty() {
        return None;
    }
    let operands = keys
        .iter()
        .filter_map(|k| match k {
            Key::Binding(id) => Some((*id, operand(*id).unwrap())),
            _ => None,
        })
        .collect();
    Some(Plan {
        body,
        keys,
        operands,
        constructed,
    })
}

/// The functions the module constructs by name: `new f(...)` of a declared
/// function, and of a binding a function expression initialises or is
/// assigned to (declared function ids, closure ids). Only their bodies arm a
/// run: a function only ever called as a method was never the birth of its
/// receivers, and a run's check would cost its every call and split the
/// receiver run of its stores (`layout_read::RecvRun`).
fn constructed(hir: &Module) -> (HashSet<u32>, HashSet<u32>) {
    let mut functions = HashSet::new();
    let mut locals = HashSet::new();
    let bound: std::cell::RefCell<HashMap<u32, u32>> = Default::default();
    for body in crate::layout_families::module_bodies(hir) {
        crate::layout_families::visit_stmts(
            body,
            &mut |s| {
                if let Stmt::Let {
                    id,
                    init: Some(Expr::Closure { func_id, .. }),
                    ..
                } = s
                {
                    bound.borrow_mut().insert(*id, *func_id);
                }
            },
            &mut |e| match e {
                Expr::NewDynamic { callee, .. } | Expr::NewDynamicSpread { callee, .. } => {
                    match callee.as_ref() {
                        Expr::FuncRef(id) => {
                            functions.insert(*id);
                        }
                        Expr::LocalGet(id) => {
                            locals.insert(*id);
                        }
                        _ => {}
                    }
                }
                Expr::LocalSet(id, value) => {
                    if let Expr::Closure { func_id, .. } = value.as_ref() {
                        bound.borrow_mut().insert(*id, *func_id);
                    }
                }
                _ => {}
            },
        );
    }
    let bound = bound.into_inner();
    let closures = locals
        .iter()
        .filter_map(|l| bound.get(l).copied())
        .collect();
    (functions, closures)
}
pub(crate) fn collect(
    hir: &Module,
    prefix_name: &str,
    globals: &HashMap<u32, String>,
    closures: &[(u32, Expr)],
    boxed: &HashSet<u32>,
    scopes: &crate::scope_env::ScopeMap,
    enabled: bool,
) {
    PLANS.with(|p| p.borrow_mut().clear());
    if !enabled {
        return;
    }
    let global = |id: u32| {
        globals.get(&id).map(|g| Operand {
            global: Some(g.clone()),
            capture: 0,
            kind: 3,
            slot: 0,
        })
    };
    let (constructed_functions, constructed_closures) = constructed(hir);
    let mut plans = Vec::new();
    for f in &hir.functions {
        if !f.is_async && !f.is_generator {
            if let Some(p) = plan(
                format!(
                    "__perry_wrap_{}",
                    crate::codegen::user_function_symbol(&hir.name, &f.name)
                ),
                &f.body,
                constructed_functions.contains(&f.id),
                &global,
            ) {
                plans.push(p);
            }
        }
    }
    for (id, expr) in closures {
        if let Expr::Closure {
            params,
            body,
            captures,
            is_arrow: false,
            is_async: false,
            is_generator: false,
            ..
        } = expr
        {
            let captures = crate::type_analysis::compute_auto_captures_with_globals(
                params, body, captures, globals, scopes,
            );
            let positions: HashMap<_, _> = captures
                .iter()
                .enumerate()
                .map(|(i, id)| (*id, i as u32))
                .collect();
            let operand = |id: u32| {
                if let Some(g) = global(id) {
                    return Some(g);
                }
                let capture = *positions.get(&scopes.capture_key(id))?;
                let slot = scopes.slot(id);
                Some(Operand {
                    global: None,
                    capture,
                    kind: if slot.is_some() || boxed.contains(&id) {
                        2
                    } else {
                        1
                    },
                    slot: slot.map_or(0, |s| s.index as u16),
                })
            };
            if let Some(p) = plan(
                crate::fn_info::closure_body_symbol(prefix_name, *id),
                body,
                constructed_closures.contains(id),
                operand,
            ) {
                plans.push(p);
            }
        }
    }
    PLANS.with(|p| *p.borrow_mut() = plans);
}

pub(crate) fn emit(
    llmod: &mut LlModule,
    strings: &mut StringPool,
    atoms: &mut BTreeMap<String, String>,
) {
    let plans = PLANS.with(|p| p.borrow().clone());
    let atom_sect = section(
        &llmod.target_triple,
        "perry_atoms",
        "__DATA,__perry_atoms",
        ".perryatm$m",
    );
    for Plan {
        body,
        keys,
        operands: sources,
        ..
    } in plans
    {
        if keys.is_empty() {
            continue;
        }
        let desc = format!("{body}$ctor_layout");
        let mut operands = Vec::new();
        let mut entries = Vec::new();
        for (slot, key) in keys.iter().enumerate() {
            let a = match key {
                Key::String(k) => {
                    if let Some(a) = atoms.get(k) {
                        a.clone()
                    } else {
                        let name = format!("{desc}$string{slot}");
                        let idx = strings.intern(k);
                        let image = crate::codegen::string_pool_image_pointer(strings.entry(idx));
                        llmod.add_raw_global(format!("@{name} = private constant {ATOM_TY} {{ i64 {}, ptr {image}, i8 0, [3 x i8] zeroinitializer, i32 0 }}, align 8", super::fnv1a(k.as_bytes()) as i64));
                        name
                    }
                }
                Key::Binding(id) => {
                    let name = format!("{desc}$key{slot}");
                    let atom_storage = super::atom_storage(&llmod.target_triple);
                    llmod.add_raw_global(format!("@{name} = private {atom_storage} {ATOM_TY} {{ i64 0, ptr null, i8 {}, [3 x i8] zeroinitializer, i32 {slot} }}, section \"{atom_sect}\", align 8", crate::runtime_abi::ATOM_KIND_BINDING_SYMBOL));
                    let source = &sources[id];
                    // A metadata constant cannot hold the address of a TLS
                    // binding for every agent. This immutable reader is one
                    // raw load from the existing binding, with no JS/TDZ check.
                    let global = if let Some(g) = &source.global {
                        let reader = format!("{g}$ctor_operand");
                        if !llmod.has_function(&reader) {
                            let f = llmod.define_function(&reader, crate::types::I64, vec![]);
                            f.linkage = "internal".into();
                            let b = f.create_block("entry");
                            let bits = b.load(crate::types::I64, &format!("@{g}"));
                            b.ret(crate::types::I64, &bits);
                        }
                        format!("@{reader}")
                    } else {
                        "null".into()
                    };
                    operands.push(format!(
                        "{{ ptr, i32, i16, i16 }} {{ ptr {global}, i32 {}, i16 {}, i16 {} }}",
                        source.capture, source.kind, source.slot
                    ));
                    name
                }
            };
            entries.push(format!(
                "{KEY_TY} {{ ptr @{a}, i32 {slot}, i8 -128, i8 0, i16 {} }}",
                super::key_off(
                    &llmod.target_triple,
                    crate::target_layout::HeaderForm::Meta,
                    slot as u32
                )
            ));
        }
        // All keys are optional, so birth exposes no property. The actual
        // prototype is bound as an ordinary shape fact at the first admitted
        // birth; a different current prototype declines at construction.
        let count = keys.len();
        let cap = (count * 2).next_power_of_two().max(2);
        let mut index = vec![u32::MAX; cap];
        for (i, key) in keys.iter().enumerate() {
            let hash = match key {
                Key::String(k) => super::fnv1a(k.as_bytes()),
                Key::Binding(_) => 0,
            };
            let mut at = hash as usize & (cap - 1);
            while index[at] != u32::MAX {
                at = (at + 1) & (cap - 1);
            }
            index[at] = i as u32;
        }
        let index_items: Vec<_> = index.iter().map(|i| format!("i32 {}", *i as i32)).collect();
        llmod.add_raw_global(format!(
            "@{desc}$index = private constant [{cap} x i32] [{}]",
            index_items.join(", ")
        ));
        llmod.add_raw_global(format!(
            "@{desc}$keys = private constant [{count} x {KEY_TY}] [{}], align 8",
            entries.join(", ")
        ));
        let word = crate::layout_ordinals::descriptor_word(
            &desc,
            format!("{keys:?}|{count}|{operands:?}"),
            0,
            false,
            false,
            0,
        );
        llmod.add_raw_global(format!("@{desc} = global {DESC_TY} {{ i32 {}, i16 {}, i8 {}, i8 0, i32 {count}, i32 {}, i64 0, ptr inttoptr (i64 2 to ptr), ptr @{desc}$keys, i32 {count}, i32 {}, ptr @{desc}$index, i32 0, i32 0, ptr null, i32 0, i32 0, ptr null, {} }}, align 8", super::fnv1a(desc.as_bytes()) as u32 & 0x7fff_ffff, crate::runtime_abi::LAYOUT_VARIANT_FUNCTION_CONSTRUCTOR, 0u8, word as i32, cap - 1, super::desc_tail(&llmod.target_triple, crate::target_layout::HeaderForm::Meta, count as u32)));
        let n = operands.len();
        llmod.add_raw_global(format!(
            "@{desc}$operands = private constant [{n} x {{ ptr, i32, i16, i16 }}] [{}]",
            operands.join(", ")
        ));
        llmod.note_fn_info(&body, |f| {
            f.constructor_layout = Some(crate::fn_info::ConstructorLayout {
                descriptor: desc.clone(),
                operands: format!("{desc}$operands"),
                symbols: n as u32,
            })
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(feature = "llvm-inprocess")]
    #[test]
    fn a_bound_constructor_descriptor_parses_with_the_current_layout_abi() {
        let old = PLANS.with(|p| {
            p.replace(vec![Plan {
                body: "ctor".into(),
                keys: vec![Key::Binding(7)],
                operands: HashMap::from([(
                    7,
                    Operand {
                        global: None,
                        capture: 0,
                        kind: 1,
                        slot: 0,
                    },
                )]),
                constructed: true,
            }])
        });
        let mut module = LlModule::new("x86_64-unknown-linux-gnu");
        emit(&mut module, &mut StringPool::new(), &mut BTreeMap::new());
        PLANS.with(|p| p.replace(old));
        let context = inkwell::context::Context::create();
        crate::inprocess::parse_ir_text(&context, &module.to_ir(), "constructor_layout")
            .expect("a per-evaluation constructor descriptor must have every ABI field");
    }

    #[test]
    fn only_straight_line_this_stores_form_the_prefix() {
        let set = |k: &str| {
            Stmt::Expr(Expr::PropertySet {
                object: Box::new(Expr::This),
                property: k.into(),
                value: Box::new(Expr::Undefined),
                byte_offset: 0,
                byte_end: 0,
            })
        };
        assert_eq!(
            prefix(
                &[set("a"), set("b"), Stmt::Return(None, 0), set("c")],
                &|_| { false }
            ),
            vec![Key::String("a".into()), Key::String("b".into())]
        );
        assert!(prefix(&[set("__proto__")], &|_| false).is_empty());
        assert_eq!(
            prefix(&[Stmt::PreallocateBoxes(vec![7]), set("a")], &|_| false),
            vec![Key::String("a".into())]
        );
        let captured = Stmt::Expr(Expr::PutValueSet {
            target: Box::new(Expr::This),
            receiver: Box::new(Expr::This),
            key: Box::new(Expr::LocalGet(7)),
            value: Box::new(Expr::Undefined),
            strict: true,
            element_offset: 0,
            byte_offset: 0,
            byte_end: 0,
        });
        assert_eq!(
            prefix(&[captured.clone()], &|id| id == 7),
            vec![Key::Binding(7)]
        );
        assert!(prefix(&[captured], &|_| false).is_empty());
    }

    #[test]
    fn only_a_function_the_module_constructs_arms_a_run() {
        let closure = |func_id: u32| Expr::Closure {
            byte_offset: 0,
            func_id,
            params: Vec::new(),
            return_type: perry_hir::types::Type::Any,
            body: Vec::new(),
            captures: Vec::new(),
            mutable_captures: Vec::new(),
            captures_this: false,
            captures_new_target: false,
            enclosing_class: None,
            is_arrow: false,
            is_async: false,
            is_generator: false,
            is_strict: false,
            construct_only: false,
        };
        let new = |callee: Expr| {
            Stmt::Expr(Expr::NewDynamic {
                callee: Box::new(callee),
                args: Vec::new(),
                byte_offset: 0,
                byte_end: 0,
            })
        };
        let mut m = Module::new("ctors");
        m.init = vec![
            // var B = function () {}; var C; C = function () {};
            Stmt::Let {
                id: 1,
                name: "B".into(),
                ty: perry_hir::types::Type::Any,
                mutable: true,
                init: Some(closure(21)),
            },
            Stmt::Expr(Expr::LocalSet(2, Box::new(closure(22)))),
            // var M = function () {}: only ever called, never constructed.
            Stmt::Let {
                id: 3,
                name: "M".into(),
                ty: perry_hir::types::Type::Any,
                mutable: true,
                init: Some(closure(23)),
            },
            new(Expr::FuncRef(10)),
            new(Expr::LocalGet(1)),
            new(Expr::LocalGet(2)),
        ];
        let (functions, closures) = constructed(&m);
        assert_eq!(functions, HashSet::from([10]));
        assert_eq!(closures, HashSet::from([21, 22]));
    }

    #[test]
    fn a_chained_store_lists_its_keys_in_the_order_it_adds_them() {
        let store = |k: &str, value: Expr| Expr::PutValueSet {
            target: Box::new(Expr::This),
            receiver: Box::new(Expr::This),
            key: Box::new(Expr::String(k.into())),
            value: Box::new(value),
            strict: false,
            element_offset: 0,
            byte_offset: 0,
            byte_end: 0,
        };
        // this.tag = 1; this.a = this.b = this.c = null; this.d = 0
        let chain = store("a", store("b", store("c", Expr::Null)));
        let body = [
            Stmt::Expr(store("tag", Expr::Integer(1))),
            Stmt::Expr(chain),
            Stmt::Expr(store("d", Expr::Integer(0))),
        ];
        let keys: Vec<Key> = ["tag", "c", "b", "a", "d"]
            .iter()
            .map(|k| Key::String((*k).into()))
            .collect();
        assert_eq!(prefix(&body, &|_| false), keys);
        // A nested key the prefix cannot hold ends it before the outer key.
        let body = [
            Stmt::Expr(store("tag", Expr::Integer(1))),
            Stmt::Expr(store("a", store("__proto__", Expr::Null))),
            Stmt::Expr(store("d", Expr::Integer(0))),
        ];
        assert_eq!(prefix(&body, &|_| false), vec![Key::String("tag".into())]);
    }
}

/// `f` of the constructor layout plan of the body being lowered, if it has one.
fn current_plan<T>(ctx: &crate::expr::FnCtx<'_>, f: impl Fn(&Plan) -> Option<T>) -> Option<T> {
    let source_body = ctx
        .func
        .name
        .strip_suffix("$this")
        .unwrap_or(&ctx.func.name);
    PLANS.with(|p| {
        p.borrow().iter().find_map(|plan| {
            if plan.body != source_body
                && plan.body.strip_prefix("__perry_wrap_") != Some(source_body)
            {
                return None;
            }
            f(plan)
        })
    })
}

/// Does evaluating `value`, a store's right-hand side in a constructor whose
/// layout names `keys`, run no user code once a run's check holds? A value
/// [`run_value`](crate::expr::layout_read::run_value) calls free, or a
/// chained `this.k = <such a value>` of a named key `k`, whose store is
/// itself a member of the run.
fn run_value_free(value: &Expr, keys: &[&str]) -> bool {
    use crate::expr::layout_read::{run_value, RunValue};
    match value {
        Expr::PropertySet {
            object,
            property,
            value,
            ..
        } if matches!(object.as_ref(), Expr::This) => {
            keys.contains(&property.as_str()) && run_value_free(value, keys)
        }
        Expr::PutValueSet {
            target,
            receiver,
            key,
            value,
            ..
        } if matches!(target.as_ref(), Expr::This) && matches!(receiver.as_ref(), Expr::This) => {
            matches!(key.as_ref(), Expr::String(k) if keys.contains(&k.as_str()))
                && run_value_free(value, keys)
        }
        _ => run_value(value, keys) == RunValue::Free,
    }
}

/// The run arm of `this.<property> = value` in an ordinary function
/// constructor's body, lowered after its value (object model step 2c, as a
/// class constructor's `layout_read::family_run_arm`): the store fills the
/// key's slot of the constructor's birth layout in place while the run's one
/// check (`perry_function_run_admits`) holds. A store whose value runs no
/// user code continues the run in flight; any other asks anew. A store of a
/// key the layout does not name, or whose site is the generic one, ends the
/// run: its add could move the receiver off the layout.
pub(crate) fn run_arm(
    ctx: &mut crate::expr::FnCtx<'_>,
    this_bits: &str,
    property: &str,
    value: &Expr,
) -> Option<crate::expr::layout_read::RunArm> {
    use crate::expr::layout_read::{FamilyRun, RunArm};
    use crate::types::{I32, I64, PTR};
    let run_in = ctx.fn_ctor_run.take();
    let (desc, pos, free) = current_plan(ctx, |plan| {
        let Plan { body, keys, .. } = plan;
        if !plan.constructed {
            return None;
        }
        let pos = keys
            .iter()
            .position(|k| *k == Key::String(property.to_owned()))?;
        let names: Vec<&str> = keys
            .iter()
            .filter_map(|k| match k {
                Key::String(s) => Some(s.as_str()),
                Key::Binding(_) => None,
            })
            .collect();
        Some((
            format!("{body}$ctor_layout"),
            pos as u32,
            run_value_free(value, &names),
        ))
    })?;
    let carried = run_in.filter(|r| free && ctx.proof_dominates_here(&r.ok));
    if let Some(r) = &carried {
        // A key the run already stored: the site overwrites its data.
        if pos <= r.last_pos {
            ctx.fn_ctor_run = carried;
            return None;
        }
    }
    let ok = match carried {
        Some(r) => r.ok,
        None => {
            let (dir, _key_bits, _hash) = crate::expr::method_site::emit_site_key(ctx, property);
            let c = ctx.block().call(
                I32,
                "perry_function_run_admits",
                &[
                    (PTR, &dir),
                    (PTR, &format!("@{desc}")),
                    (I64, this_bits),
                    (I32, &pos.to_string()),
                ],
            );
            ctx.block().icmp_ne(I32, &c, "0")
        }
    };
    ctx.fn_ctor_run = Some(FamilyRun {
        ok: ok.clone(),
        last_pos: pos,
    });
    Some(RunArm {
        ok,
        slot: pos,
        key: property.to_owned(),
        function_off: Some(super::key_off(
            ctx.target_triple,
            crate::target_layout::HeaderForm::Meta,
            pos,
        )),
    })
}

/// A store in the ordinary body is used by construction and borrowed calls.
/// Guard the current receiver and key after evaluating the RHS; a miss takes
/// the complete PutValue path without re-evaluating any operand.
pub(crate) fn lower_symbol_store(
    ctx: &mut crate::expr::FnCtx<'_>,
    object: &Expr,
    index: &Expr,
    value: &Expr,
    strict: bool,
) -> anyhow::Result<Option<String>> {
    use crate::types::{DOUBLE, I32, PTR};
    if !matches!(object, Expr::This) {
        return Ok(None);
    }
    let Expr::LocalGet(id) = index else {
        return Ok(None);
    };
    let site = current_plan(ctx, |Plan { body, keys, .. }| {
        let pos = keys.iter().position(|k| k == &Key::Binding(*id))?;
        Some((format!("{body}$ctor_layout"), pos))
    });
    let Some((desc, pos)) = site else {
        return Ok(None);
    };
    crate::rooting::with_operands_rooted(ctx, &[object, index, value], |ctx, values| {
        // Keep the evaluated key across the RHS. PutValue performs any key
        // coercion after the RHS, as the pinned Node oracle requires.
        Ok(Some(ctx.block().call(
            DOUBLE,
            "perry_function_family_symbol_set",
            &[
                (PTR, &format!("@{desc}")),
                (I32, &pos.to_string()),
                (DOUBLE, &values[0]),
                (DOUBLE, &values[1]),
                (DOUBLE, &values[2]),
                (I32, if strict { "1" } else { "0" }),
            ],
        )))
    })
}
