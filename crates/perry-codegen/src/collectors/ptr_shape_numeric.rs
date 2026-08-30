//! Pass 4 of `collectors/ptr_shape.rs`: the numeric-field machinery.
//!
//! Four pieces, one contract. A field in `numeric_fields` licenses a bare
//! `load double` that claims `JsNumber`/`F64` with **no coercion and no
//! value check**, so everything here answers the same question — "is every
//! reachable store into this slot number-producing by construction, per
//! spec?" — never "does the declared type say number" (Perry does not
//! enforce annotations).
//!
//! * [`expr_numeric_by_construction`] — the expression-level proof.
//! * [`prove_numeric_fields`] — the per-receiver reachable-store fixpoint
//!   (constructor chain, field initializers, method stores, in-function
//!   stores), with constructor/method parameters resolved through the actual
//!   argument expressions at the provenance `new`(s) / recorded call sites.
//! * [`prove_group_numeric_fields`] (#7770) — the same proof discharged once
//!   per element-shape-proven ARRAY: E1–E5 containment
//!   (`collectors/ptr_shape_elements.rs`) bounds every reference to the
//!   group's objects to the group's members and the provenance `new`s at the
//!   push sites, so the union of their stores is exhaustive and the meet
//!   over every push's constructor arguments resolves the parameter
//!   environment.
//! * [`collect_numeric_by_construction_locals`] (#7770) — locals whose every
//!   write is number-producing (loop counters above all: `let i = 0` + `i++`),
//!   so a provenance `new C(i, i + 1)` resolves. Same optimistic-fixpoint
//!   shape as `collect_not_bigint_locals`, WITHOUT its declared-type leaf:
//!   declared types stay untrusted here.
//!
//! Split out of `ptr_shape.rs` to stay under the 2000-line CI gate; declared
//! there with `#[path]` so it remains a child module and can reach the
//! collector's private items through `use super::*`.

use super::*;

// ── Parameter environments ─────────────────────────────────────────────────

/// Parameter environment for [`expr_numeric_by_construction`].
pub(super) enum ParamEnv<'x> {
    /// Function scope: no parameters; const-local chasing applies.
    None,
    /// Method scope: params resolve through recorded call-site argument
    /// lists (each argument evaluated in function scope).
    Sites {
        param_ids: &'x [u32],
        sites: Vec<&'x [Expr]>,
    },
    /// Constructor scope: params pre-resolved to a numeric verdict through
    /// the provenance `new` / `super(...)` argument chain.
    Resolved(&'x HashMap<u32, bool>),
}

// ── The per-receiver reachable-store fixpoint ──────────────────────────────

/// Greatest-fixpoint proof that every reachable store into a raw-f64-declared
/// chain field is number-producing. Parameter-mediated stores resolve through
/// the actual argument expressions at the provenance `new`(s) (constructor)
/// or at every recorded call site (methods).
///
/// `new_arg_lists` carries ONE argument list per provenance `new`. A single
/// rule-1 candidate has exactly one; an element group (#7770) has one per
/// push site, and a constructor parameter is numeric only when EVERY list
/// proves it — the meet. An empty slice means the provenance is unresolved
/// and every parameter stays unproven.
#[allow(clippy::too_many_arguments)]
pub(super) fn prove_numeric_fields(
    chain: &[&Class],
    members: &HashSet<u32>,
    this_stores: &[ThisStoreRecord<'_>],
    local_stores: &[(String, StoreValue<'_>)],
    new_arg_lists: &[&[Expr]],
    method_calls: Option<&HashMap<String, Vec<&[Expr]>>>,
    super_call_args: &HashMap<String, Vec<&[Expr]>>,
    internally_invoked: &HashSet<String>,
    not_bigint_locals: &HashSet<u32>,
    const_local_inits: &HashMap<u32, Option<&Expr>>,
    numeric_locals: &HashSet<u32>,
) -> HashSet<String> {
    // #8619: the class-field numeric proof has no specialized-entry `TaPtr`
    // context, so no view binding is spec-proven here. Passing empty keeps this
    // proof bit-identical to before the local `TaPtr` extension.
    let no_ta_views: HashSet<u32> = HashSet::new();
    let mut numeric: HashSet<String> = HashSet::new();
    for class in chain {
        for field in &class.fields {
            if crate::typed_shape::type_is_raw_f64_candidate(&field.ty) {
                numeric.insert(field.name.clone());
            }
        }
    }
    if numeric.is_empty() {
        return numeric;
    }
    // Resolve the argument expressions that can flow into a given
    // (context, param position): the provenance `new` args feed the root
    // constructor; each parent constructor's params resolve through the
    // recorded `super(...)` argument lists, evaluated under the CALLING
    // constructor's (already-resolved) parameter environment. Derived-first
    // chain order makes this a single top-down pass. The environment is
    // computed against an EMPTY numeric-field set (strictly conservative —
    // `super(this.x)` cannot occur, `this` is banned in super args).
    let mut ctor_param_env: HashMap<String, HashMap<u32, bool>> = HashMap::new();
    {
        let empty_numeric: HashSet<String> = HashSet::new();
        for (pos, class) in chain.iter().enumerate() {
            let Some(ctor) = class.constructor.as_ref() else {
                continue;
            };
            let mut env: HashMap<u32, bool> = HashMap::new();
            if pos == 0 {
                for (i, param) in ctor.params.iter().enumerate() {
                    // Meet over every provenance `new`: a missing argument is
                    // `undefined`, so it fails; no lists at all proves
                    // nothing.
                    let ok = !new_arg_lists.is_empty()
                        && new_arg_lists.iter().all(|new_args| {
                            new_args
                                .get(i)
                                .map(|a| {
                                    expr_numeric_by_construction(
                                        a,
                                        &ParamEnv::None,
                                        members,
                                        &empty_numeric,
                                        not_bigint_locals,
                                        const_local_inits,
                                        numeric_locals,
                                        &no_ta_views,
                                        0,
                                    )
                                })
                                .unwrap_or(false)
                        });
                    env.insert(param.id, ok);
                }
            } else {
                let caller_env = chain
                    .get(pos - 1)
                    .and_then(|caller| ctor_param_env.get(caller.name.as_str()));
                let lists = super_call_args.get(class.name.as_str());
                for (i, param) in ctor.params.iter().enumerate() {
                    let ok = match (lists, caller_env) {
                        (Some(lists), Some(caller_env)) if !lists.is_empty() => {
                            lists.iter().all(|args| {
                                args.get(i)
                                    .map(|a| {
                                        expr_numeric_by_construction(
                                            a,
                                            &ParamEnv::Resolved(caller_env),
                                            members,
                                            &empty_numeric,
                                            not_bigint_locals,
                                            const_local_inits,
                                            numeric_locals,
                                            &no_ta_views,
                                            0,
                                        )
                                    })
                                    .unwrap_or(false)
                            })
                        }
                        _ => false,
                    };
                    env.insert(param.id, ok);
                }
            }
            ctor_param_env.insert(class.name.clone(), env);
        }
    }

    loop {
        let before = numeric.len();
        let is_store_numeric = |field: &str,
                                value: Option<&Expr>,
                                context: Option<&(String, String, Vec<u32>)>,
                                numeric: &HashSet<String>|
         -> bool {
            let _ = field;
            let Some(value) = value else {
                // `++`/`--` — ToNumeric of a proven-number field stays a
                // number; if the field is currently claimed numeric the
                // update preserves it.
                return true;
            };
            let param_env: ParamEnv<'_> = match context {
                None => ParamEnv::None,
                Some((owner, name, param_ids)) => {
                    if name == "constructor" {
                        match ctor_param_env.get(owner.as_str()) {
                            Some(env) => ParamEnv::Resolved(env),
                            None => ParamEnv::Sites {
                                param_ids: param_ids.as_slice(),
                                sites: Vec::new(),
                            },
                        }
                    } else {
                        // A method that is ALSO invoked internally
                        // (`this.m(...)` / `super.m(...)`) receives argument
                        // expressions from method scope that the
                        // function-scope site resolution below cannot see —
                        // its parameters stay unproven even when every
                        // external site is numeric (an internal
                        // `this.m("s")` would otherwise poison a
                        // "proven" field). Purely-external methods resolve
                        // through their recorded call sites; purely-internal
                        // ones have no sites and stay unproven either way.
                        let sites: Vec<&[Expr]> = if internally_invoked.contains(name.as_str()) {
                            Vec::new()
                        } else {
                            method_calls
                                .and_then(|mc| mc.get(name))
                                .map(|v| v.clone())
                                .unwrap_or_default()
                        };
                        ParamEnv::Sites {
                            param_ids: param_ids.as_slice(),
                            sites,
                        }
                    }
                }
            };
            expr_numeric_by_construction(
                value,
                &param_env,
                members,
                numeric,
                not_bigint_locals,
                const_local_inits,
                numeric_locals,
                &no_ta_views,
                0,
            )
        };
        // Field initializers + ctor/method stores.
        let mut retained: HashSet<String> = numeric.clone();
        for rec in this_stores {
            if retained.contains(&rec.field)
                && !is_store_numeric(&rec.field, rec.value, rec.context.as_ref(), &numeric)
            {
                retained.remove(&rec.field);
            }
        }
        for (field, sv) in local_stores {
            if retained.contains(field) {
                let ok = match sv {
                    StoreValue::Update => true,
                    StoreValue::Direct(v) => expr_numeric_by_construction(
                        v,
                        &ParamEnv::None,
                        members,
                        &numeric,
                        not_bigint_locals,
                        const_local_inits,
                        numeric_locals,
                        &no_ta_views,
                        0,
                    ),
                };
                if !ok {
                    retained.remove(field);
                }
            }
        }
        numeric = retained;
        if numeric.len() == before || numeric.is_empty() {
            break;
        }
    }
    numeric
}

// ── #7770: the per-element-group proof ─────────────────────────────────────

/// Discharge the reachable-store proof once per element-shape-proven array.
///
/// Soundness rests on the same containment that makes the SHAPE proof valid
/// (`collectors/ptr_shape_elements.rs`, E1–E5): while the facts hold, every
/// reference to a group's objects is a vetted producer local, a licensed
/// element-read local, or the inline `new C(...)` at a push site — an
/// unlicensed element read disqualifies the array, and `A[i].f = v` goes
/// through an unlicensed `IndexGet`, so neither can coexist with the facts.
/// The reachable-store set is therefore exactly:
///
/// * the constructor chain + field initializers, with the parameter
///   environment resolved as the MEET over every push's `new` argument list;
/// * every member's in-function field stores;
/// * every method invoked on any member, with call sites merged group-wide.
///
/// The result is keyed by array root; every member of a surviving group
/// carries the same set. Claims stay honest through group integrity: any
/// member failing rule 2 drops every member's fact, claim included. Returns
/// no entry (never a partial one) whenever any obligation fails — the
/// members then simply claim nothing, exactly the pre-#7770 stand-down.
#[allow(clippy::too_many_arguments)]
pub(super) fn prove_group_numeric_fields<'a>(
    classes: &HashMap<String, &'a Class>,
    module_dispatch: &ModuleDispatchFacts,
    element_facts: &ElementShapeFacts,
    groups: &HashMap<u32, Vec<u32>>,
    roots: &HashMap<u32, u32>,
    field_stores: &HashMap<u32, Vec<(String, StoreValue<'a>)>>,
    method_calls: &HashMap<u32, HashMap<String, Vec<&'a [Expr]>>>,
    new_args: &HashMap<u32, &'a [Expr]>,
    element_pushes: &HashMap<u32, Vec<ElementPush<'a>>>,
    not_bigint_locals: &HashSet<u32>,
    const_local_inits: &HashMap<u32, Option<&'a Expr>>,
    numeric_locals: &HashSet<u32>,
) -> HashMap<u32, HashSet<String>> {
    let mut out: HashMap<u32, HashSet<String>> = HashMap::new();
    'group: for (&root, members) in groups {
        let Some(class_name) = element_facts.root_class(root) else {
            continue;
        };
        let chain = chain_classes(classes, class_name);
        if chain.is_empty() {
            continue;
        }
        // Nothing raw-f64-declared anywhere on the chain means there is no
        // claim to make — skip the whole this-flow walk for such groups
        // (proven arrays of string/object-only records are common).
        if !chain.iter().any(|c| {
            c.fields
                .iter()
                .any(|f| crate::typed_shape::type_is_raw_f64_candidate(&f.ty))
        }) {
            continue;
        }
        let fields = chain_field_names(&chain);
        let methods = chain_method_map(&chain);
        // Merge method call sites group-wide: a method's parameter is numeric
        // only when every site on every member passes a numeric argument.
        let mut merged_calls: HashMap<String, Vec<&'a [Expr]>> = HashMap::new();
        for m in members {
            if let Some(mc) = method_calls.get(m) {
                for (name, sites) in mc {
                    merged_calls
                        .entry(name.clone())
                        .or_default()
                        .extend_from_slice(sites);
                }
            }
        }
        // The same obligations the `'cand` loop imposes before it trusts a
        // method walk — literally the same function, so the group claim can
        // never rest on a weaker basis than the per-candidate proof.
        let Ok(mut analysis) = super::chain_this_flow_verdict(
            classes,
            module_dispatch,
            class_name,
            &chain,
            &fields,
            &methods,
            Some(&merged_calls),
        ) else {
            continue 'group;
        };
        // One argument list per push — ALL of them, or no claim. A producer
        // whose `new_args` went unrecorded, or a push shape E2 would never
        // have admitted, forfeits the group's claim rather than narrowing
        // the meet.
        let mut new_lists: Vec<&'a [Expr]> = Vec::new();
        for push in element_pushes.get(&root).map(Vec::as_slice).unwrap_or(&[]) {
            match push {
                ElementPush::Inline(args) => new_lists.push(args),
                ElementPush::Producer(v) => match new_args.get(v) {
                    Some(args) => new_lists.push(args),
                    None => continue 'group,
                },
                ElementPush::Opaque => continue 'group,
            }
        }
        if new_lists.is_empty() {
            continue;
        }
        // The fixpoint's member set: every group member plus every alias of
        // one — `r2.x` read as a store value proves through the same set.
        let member_set: HashSet<u32> = members
            .iter()
            .copied()
            .chain(
                roots
                    .iter()
                    .filter(|(_, r)| members.contains(r))
                    .map(|(m, _)| *m),
            )
            .collect();
        let mut merged_stores: Vec<(String, StoreValue<'a>)> = Vec::new();
        for m in members {
            if let Some(fs) = field_stores.get(m) {
                merged_stores.extend(fs.iter().cloned());
            }
        }
        let store_records = std::mem::take(&mut analysis.store_records);
        let super_call_args = std::mem::take(&mut analysis.super_call_args);
        let internally_invoked = std::mem::take(&mut analysis.internally_invoked);
        let numeric = prove_numeric_fields(
            &chain,
            &member_set,
            &store_records,
            &merged_stores,
            &new_lists,
            Some(&merged_calls),
            &super_call_args,
            &internally_invoked,
            not_bigint_locals,
            const_local_inits,
            numeric_locals,
        );
        if !numeric.is_empty() {
            out.insert(root, numeric);
        }
    }
    out
}

// ── #7770: numeric-by-construction locals ──────────────────────────────────

/// Locals whose every write is number-producing by construction — above all
/// the loop counter (`let i = 0` + `i++`) that feeds a provenance
/// `new C(i, i + 1)`.
///
/// Candidates are ids bound by at least one `Stmt::Let` in the region —
/// params and `catch` bindings have no `Let`, which is what keeps
/// caller-controlled values out. Boxed and module-global ids are excluded:
/// their write sets are not this region's to enumerate. A `Let` with no
/// initializer poisons (the binding is `undefined` until assigned — the same
/// verdict `Expr::Undefined` gets as a store value). `Update` (`++`/`--`)
/// needs no record: ToNumeric of a value every checked write proved to be a
/// Number is that number.
///
/// Optimistic greatest fixpoint, converging downward exactly like
/// `collect_not_bigint_locals`: a self-referencing write (`s = s + x`)
/// short-circuits on the running assumption, and a TDZ self-reference
/// (`let x = x`) is vacuously sound — the read throws, so no value is ever
/// stored or observed.
pub(in crate::collectors) fn collect_numeric_by_construction_locals<'a>(
    stmts: &'a [Stmt],
    boxed_vars: &HashSet<u32>,
    module_globals: &HashMap<u32, String>,
    not_bigint_locals: &HashSet<u32>,
    const_local_inits: &HashMap<u32, Option<&'a Expr>>,
    // #8619: view bindings proven to hold a numeric-kind typed array (spec-ABI
    // `TaPtr` params). Empty for the `Ptr<Shape>` type-analysis caller.
    numeric_ta_views: &HashSet<u32>,
) -> HashSet<u32> {
    // ONE write walker for both fixpoints (`collect_not_bigint_locals` and
    // this one) — see its doc for why sharing is load-bearing. `None` = a
    // no-init `Let`, which THIS consumer treats as fatal (`undefined` is not
    // a number) where the non-BigInt one treats it as fine.
    let mut writes: HashMap<u32, Vec<Option<&'a Expr>>> = HashMap::new();
    let mut let_bound: HashSet<u32> = HashSet::new();
    super::super::not_bigint_locals::collect_writes(stmts, &mut writes, &mut let_bound);
    // The standalone #8105 consumer does not run the Ptr<Shape> provenance
    // walk that normally supplies `const_local_inits`. Reconstruct the same
    // safe fact from the shared exhaustive write set: one initialized write
    // means the binding's value is stable even when its source spelling was
    // `let`. This lets the Add proof inspect compiler-owned typed-view
    // constructors without trusting their erased annotation.
    let mut stable_local_inits = const_local_inits.clone();
    for (&id, local_writes) in &writes {
        if let [Some(init)] = local_writes.as_slice() {
            stable_local_inits.entry(id).or_insert(Some(*init));
        }
    }
    let empty_members: HashSet<u32> = HashSet::new();
    let empty_fields: HashSet<String> = HashSet::new();
    let mut numeric: HashSet<u32> = let_bound
        .into_iter()
        .filter(|id| !boxed_vars.contains(id) && !module_globals.contains_key(id))
        .collect();
    loop {
        let mut drop: Vec<u32> = Vec::new();
        for &id in &numeric {
            let ok = writes
                .get(&id)
                .map(|ws| {
                    ws.iter().all(|w| match w {
                        None => false,
                        Some(e) => expr_numeric_by_construction(
                            e,
                            &ParamEnv::None,
                            &empty_members,
                            &empty_fields,
                            not_bigint_locals,
                            &stable_local_inits,
                            &numeric,
                            numeric_ta_views,
                            0,
                        ),
                    })
                })
                // A `let_bound` id always has its `Let` recorded; treat a
                // missing entry as unproven rather than as vacuously true.
                .unwrap_or(false);
            if !ok {
                drop.push(id);
            }
        }
        if drop.is_empty() {
            break;
        }
        for id in drop {
            numeric.remove(&id);
        }
    }
    numeric
}

// ── The expression-level proof ─────────────────────────────────────────────

/// Number-by-construction: the expression's runtime value is a JS Number for
/// every input, per spec — never a string/BigInt/bool/undefined/pointer.
#[allow(clippy::too_many_arguments)]
pub(super) fn expr_numeric_by_construction(
    e: &Expr,
    param_env: &ParamEnv<'_>,
    members: &HashSet<u32>,
    numeric_fields: &HashSet<String>,
    not_bigint_locals: &HashSet<u32>,
    const_local_inits: &HashMap<u32, Option<&Expr>>,
    numeric_locals: &HashSet<u32>,
    // #8619: view bindings PROVEN to permanently hold a numeric-kind typed
    // array — a spec-ABI `TaPtr` parameter (the entry contract binds the raw
    // header of a proven numeric non-view typed array). A read
    // `view_id[numeric_index]` is then a Number (in-bounds) or `undefined`
    // (OOB) by construction, never a pointer/string, which the Add rule below
    // launders into a genuine Number. Empty on every path that is not a
    // specialized-entry local proof (the class-field provers, the `Ptr<Shape>`
    // pass).
    numeric_ta_views: &HashSet<u32>,
    depth: usize,
) -> bool {
    if depth > 16 {
        return false;
    }
    use perry_hir::BinaryOp;
    let rec = |x: &Expr| {
        expr_numeric_by_construction(
            x,
            param_env,
            members,
            numeric_fields,
            not_bigint_locals,
            const_local_inits,
            numeric_locals,
            numeric_ta_views,
            depth + 1,
        )
    };
    // A numeric index into one of these compiler-owned constructors can only
    // produce a Number or `undefined` (for an out-of-bounds read). Either is
    // safe on one side of `+` once the other operand has the same property:
    // neither operand can select string concatenation, and ToNumber(undefined)
    // produces the Number NaN. Keep this weaker fact local to the Add rule;
    // an out-of-bounds read is not itself a Number and must not become a
    // general raw-f64 proof.
    let numeric_view_value_or_undefined = |x: &Expr| {
        // The HIR lowers `u8[i]` on a binding it already knows is a
        // `Uint8Array`/`Buffer` to the dedicated `Uint8ArrayGet` node
        // (`lower/expr_member/member_tail.rs`), so a `const px = new
        // Uint8Array(n)` receiver — the very binding this rule is written
        // for — never arrived here as `IndexGet`, and `acc += px[i]` fell
        // out of the fixpoint: every `+=` lowered through
        // `js_dynamic_string_or_number_add` with `acc` as a rooted slot
        // (bench_int_arithmetic: 7.7x node; `acc += (px[i] | 0)` was 2000x
        // faster than `acc += px[i]`). Both nodes read the same storage.
        let (object, index) = match x {
            Expr::IndexGet { object, index } => (object, index),
            Expr::Uint8ArrayGet { array, index } => (array, index),
            _ => return false,
        };
        let Expr::LocalGet(view_id) = object.as_ref() else {
            return false;
        };
        // #8619: a spec-proven numeric typed-array binding (`TaPtr` parameter)
        // has no compiler-visible `TypedArrayNew` init in this body, but its
        // entry contract is a STRONGER proof than an inline constructor: the
        // call-site pre-pass proved the argument is one specific numeric-kind,
        // non-view typed array, never reassigned. So `view_id[numeric_index]`
        // is a Number-or-`undefined` exactly as the local-constructor case
        // below — never a pointer. The `rec(index)` guard is retained: a
        // non-numeric key (symbol/string) would read a property, which can be a
        // pointer.
        if numeric_ta_views.contains(view_id) {
            return rec(index);
        }
        let Some(Some(init)) = const_local_inits.get(view_id) else {
            return false;
        };
        let number_valued_typed_array_kind = |kind: u8| {
            matches!(
                kind,
                perry_hir::TYPED_ARRAY_KIND_INT8
                    | perry_hir::TYPED_ARRAY_KIND_UINT8
                    | perry_hir::TYPED_ARRAY_KIND_UINT8_CLAMPED
                    | perry_hir::TYPED_ARRAY_KIND_INT16
                    | perry_hir::TYPED_ARRAY_KIND_UINT16
                    | perry_hir::TYPED_ARRAY_KIND_INT32
                    | perry_hir::TYPED_ARRAY_KIND_UINT32
                    | perry_hir::TYPED_ARRAY_KIND_FLOAT16
                    | perry_hir::TYPED_ARRAY_KIND_FLOAT32
                    | perry_hir::TYPED_ARRAY_KIND_FLOAT64
            )
        };
        let numeric_storage = matches!(
            init,
            Expr::BufferAlloc { .. }
                | Expr::BufferAllocUnsafe(_)
                | Expr::Uint8ArrayNew(_)
                | Expr::Uint8ArrayFrom(_)
        ) || matches!(
            init,
            Expr::TypedArrayNew { kind, .. } | Expr::NativeArenaView { kind, .. }
                if number_valued_typed_array_kind(*kind)
        ) || matches!(
            init,
            Expr::Array(elements)
                if elements
                    .iter()
                    .all(|element| matches!(element, Expr::Integer(_) | Expr::Number(_)))
        );
        numeric_storage && rec(index)
    };
    match e {
        Expr::Number(_)
        | Expr::Integer(_)
        | Expr::PodLayoutSizeOf { .. }
        | Expr::PodLayoutAlignOf { .. }
        | Expr::PodLayoutOffsetOf { .. } => true,
        Expr::Unary { op, operand } => match op {
            perry_hir::UnaryOp::Neg | perry_hir::UnaryOp::Pos | perry_hir::UnaryOp::BitNot => {
                rec(operand)
            }
            _ => false,
        },
        Expr::Binary { op, left, right } => match op {
            // `+` concatenates strings; both sides must be numbers.
            BinaryOp::Add => {
                (rec(left) || numeric_view_value_or_undefined(left))
                    && (rec(right) || numeric_view_value_or_undefined(right))
            }
            // `- * / %` produce a BigInt only for BigInt⊗BigInt; mixing a
            // BigInt with anything else THROWS (no value is stored). ONE
            // provably-non-BigInt operand therefore forces the completed
            // result onto the Number path.
            BinaryOp::Sub | BinaryOp::Mul | BinaryOp::Div | BinaryOp::Mod => {
                (rec(left) && rec(right))
                    || expr_provably_not_bigint(left, not_bigint_locals)
                    || expr_provably_not_bigint(right, not_bigint_locals)
            }
            // Same either-side argument for the BigInt-capable bitwise ops.
            BinaryOp::BitAnd
            | BinaryOp::BitOr
            | BinaryOp::BitXor
            | BinaryOp::Shl
            | BinaryOp::Shr => {
                (rec(left) && rec(right))
                    || expr_provably_not_bigint(left, not_bigint_locals)
                    || expr_provably_not_bigint(right, not_bigint_locals)
            }
            // `>>>` throws for BigInt operands; result is always a Number.
            BinaryOp::UShr => true,
            _ => false,
        },
        Expr::NumberCoerce(_)
        | Expr::ParseFloat(_)
        | Expr::ParseInt { .. }
        | Expr::MathSqrt(_)
        | Expr::MathFloor(_)
        | Expr::MathCeil(_)
        | Expr::MathRound(_)
        | Expr::MathTrunc(_)
        | Expr::MathSign(_)
        | Expr::MathAbs(_)
        | Expr::MathF16round(_)
        | Expr::MathPow(..)
        | Expr::MathMin(_)
        | Expr::MathMax(_)
        | Expr::MathMinSpread(_)
        | Expr::MathMaxSpread(_)
        | Expr::DateNow
        | Expr::PerformanceNow => true,
        // A proven-numeric field of the SAME object (fixpoint edge): `this`
        // inside the candidate's ctor/method contexts (a non-None param env),
        // or a tracked member local in function scope. A same-named field of
        // a DIFFERENT object proves nothing.
        Expr::PropertyGet {
            object, property, ..
        } if match object.as_ref() {
            Expr::This => !matches!(param_env, ParamEnv::None),
            Expr::LocalGet(id) => members.contains(id),
            _ => false,
        } =>
        {
            numeric_fields.contains(property)
        }
        Expr::Conditional {
            then_expr,
            else_expr,
            ..
        } => rec(then_expr) && rec(else_expr),
        Expr::Sequence(es) => es.last().map(|x| rec(x)).unwrap_or(false),
        // `i++` / `--i` as a VALUE: `ToNumeric(old)` (± the adjustment),
        // which is a Number unless `old` is a BigInt — exactly the
        // not-BigInt fact. (The WRITE side is judged by
        // `collect_numeric_by_construction_locals`, not here.)
        Expr::Update { id, .. } => not_bigint_locals.contains(id),
        // A parameter: numeric iff every recorded call site passes a numeric
        // argument at that position (missing argument = `undefined`, not
        // numeric). No recorded sites = unproven.
        Expr::LocalGet(id) => {
            match param_env {
                ParamEnv::Sites { param_ids, sites } => {
                    if let Some(pos) = param_ids.iter().position(|p| p == id) {
                        return !sites.is_empty()
                            && sites.iter().all(|args| {
                                args.get(pos).map(|a| {
                                    expr_numeric_by_construction(
                                        a,
                                        &ParamEnv::None,
                                        members,
                                        numeric_fields,
                                        not_bigint_locals,
                                        const_local_inits,
                                        numeric_locals,
                                        numeric_ta_views,
                                        depth + 1,
                                    )
                                }) == Some(true)
                            });
                    }
                }
                ParamEnv::Resolved(env) => {
                    if let Some(&ok) = env.get(id) {
                        return ok;
                    }
                }
                ParamEnv::None => {
                    // A single-Let const temp: chase its init (function
                    // scope, so no parameter mapping applies to it).
                    if let Some(Some(init)) = const_local_inits.get(id) {
                        return expr_numeric_by_construction(
                            init,
                            &ParamEnv::None,
                            members,
                            numeric_fields,
                            not_bigint_locals,
                            const_local_inits,
                            numeric_locals,
                            numeric_ta_views,
                            depth + 1,
                        );
                    }
                    // #7770: a local every one of whose writes is
                    // number-producing by construction — loop counters
                    // above all.
                    return numeric_locals.contains(id);
                }
            }
            false
        }
        _ => false,
    }
}

/// Conservative "cannot be a BigInt" for the spec Number-path argument.
pub(super) fn expr_provably_not_bigint(e: &Expr, not_bigint_locals: &HashSet<u32>) -> bool {
    match e {
        Expr::Number(_)
        | Expr::Integer(_)
        | Expr::String(_)
        | Expr::Bool(_)
        | Expr::PodLayoutSizeOf { .. }
        | Expr::PodLayoutAlignOf { .. }
        | Expr::PodLayoutOffsetOf { .. } => true,
        Expr::LocalGet(id) => not_bigint_locals.contains(id),
        // A `Uint8Array`/`Buffer` element read is a byte (a Number) in
        // bounds and `undefined` out of bounds — never a BigInt. This is what
        // lets `acc += px[i] * k` reach the Number path: `*` needs only ONE
        // provably-non-BigInt operand (a BigInt mixed with anything else
        // throws), so the byte read carries the whole product.
        Expr::Uint8ArrayGet { .. } | Expr::BufferIndexGet { .. } => true,
        Expr::Unary { op, operand } => match op {
            perry_hir::UnaryOp::Pos => true, // `+x` throws for BigInt
            perry_hir::UnaryOp::Neg | perry_hir::UnaryOp::BitNot => {
                expr_provably_not_bigint(operand, not_bigint_locals)
            }
            perry_hir::UnaryOp::Not => true, // `!x` is always a Boolean
        },
        Expr::Binary { .. } => false, // handled structurally by the caller
        _ => false,
    }
}

// Note on Symbol operands in the either-side non-BigInt arithmetic argument:
// ToNumber(Symbol) THROWS, so the store never completes — throw behavior is
// identical on the guarded and bare paths, and no non-number value can reach
// the slot through these operators.
