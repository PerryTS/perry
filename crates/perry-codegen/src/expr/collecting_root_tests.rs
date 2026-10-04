//! Caller roots on collecting field-store, nested-add and constructor paths.
use crate::testing::{root_slots::function_slice, temp_slots};
use crate::{compile_module, user_function_symbol};
use perry_hir::types::Type;
use perry_hir::{BinaryOp, Class, ClassField, Expr, Function, Module, Param, Stmt};
use std::collections::{BTreeMap, BTreeSet};

fn probe(params: Vec<Type>, result: Expr) -> Function {
    Function {
        id: 1,
        name: "probe".to_string(),
        type_params: Vec::new(),
        params: params
            .into_iter()
            .enumerate()
            .map(|(i, ty)| Param {
                id: i as u32 + 1,
                name: format!("p{i}"),
                ty,
                default: None,
                decorators: Vec::new(),
                is_rest: false,
                arguments_object: None,
            })
            .collect(),
        return_type: Type::Any,
        body: vec![Stmt::Return(Some(result))],
        is_async: false,
        is_generator: false,
        is_strict: true,
        is_exported: false,
        captures: Vec::new(),
        decorators: Vec::new(),
        was_plain_async: false,
        was_unrolled: false,
    }
}

fn ir_for(mut module: Module, function: Function) -> String {
    module.functions = vec![function];
    let symbol = user_function_symbol(&module.name, "probe");
    let ir = String::from_utf8(
        compile_module(&module, crate::temp_root_coverage::entry_opts())
            .expect("collecting-root fixture compiles"),
    )
    .unwrap();
    function_slice(&ir, &symbol).to_string()
}

fn blocks(ir: &str) -> BTreeMap<&str, String> {
    let mut out = BTreeMap::new();
    let mut label = None;
    for line in ir.lines() {
        if !line.starts_with(char::is_whitespace) && line.ends_with(':') {
            label = Some(line.trim_end_matches(':'));
        } else if let Some(label) = label {
            out.entry(label)
                .or_insert_with(String::new)
                .push_str(&format!("{line}\n"));
        }
    }
    out
}

#[derive(Clone, Copy, Debug)]
struct Site<'a> {
    label: &'a str,
    index: usize,
    text: &'a str,
}

struct ColdCfg<'a> {
    ir: &'a str,
    blocks: &'a BTreeMap<&'a str, String>,
    start: &'a str,
    stop: &'a str,
}

fn block<'a>(blocks: &'a BTreeMap<&str, String>, prefix: &str, ir: &str) -> (&'a str, &'a str) {
    let matches: Vec<_> = blocks
        .iter()
        .filter(|(label, _)| label.starts_with(prefix))
        .collect();
    assert_eq!(matches.len(), 1, "expected one block {prefix}:\n{ir}");
    (*matches[0].0, matches[0].1.as_str())
}

fn registers(text: &str) -> impl Iterator<Item = &str> {
    text.split(|c: char| !(c.is_alphanumeric() || c == '%' || c == '.' || c == '_'))
        .filter(|token| token.starts_with('%'))
}

fn successors(body: &str) -> impl Iterator<Item = &str> {
    // Read terminator edges only; phi predecessors are not successor edges.
    body.lines()
        .map(str::trim)
        .filter(|line| line.starts_with("br "))
        .flat_map(|line| line.split("label %").skip(1))
        .map(|tail| {
            tail.split(|c: char| !(c.is_alphanumeric() || c == '.' || c == '_'))
                .next()
                .unwrap()
        })
}

impl<'a> ColdCfg<'a> {
    fn reachable(&self, start: &'a str, skip: Option<&str>) -> BTreeSet<&'a str> {
        let mut pending = vec![start];
        let mut seen = BTreeSet::new();
        while let Some(label) = pending.pop() {
            if label == self.stop || Some(label) == skip || !seen.insert(label) {
                continue;
            }
            let body = self
                .blocks
                .get(label)
                .unwrap_or_else(|| panic!("missing CFG target {label}:\n{}", self.ir));
            pending.extend(successors(body));
        }
        seen
    }

    fn sites(&self) -> Vec<Site<'a>> {
        self.reachable(self.start, None)
            .into_iter()
            .flat_map(|label| {
                self.blocks[label]
                    .lines()
                    .enumerate()
                    .map(move |(index, text)| Site {
                        label,
                        index,
                        text: text.trim(),
                    })
            })
            .collect()
    }

    fn definition(&self, reg: &str) -> Site<'a> {
        let needle = format!("{reg} = ");
        self.blocks
            .iter()
            .find_map(|(label, body)| {
                body.lines().enumerate().find_map(|(index, text)| {
                    text.trim().starts_with(&needle).then_some(Site {
                        label,
                        index,
                        text: text.trim(),
                    })
                })
            })
            .unwrap_or_else(|| panic!("missing definition {reg}:\n{}", self.ir))
    }

    fn dominates(&self, before: Site<'_>, after: Site<'_>) -> bool {
        let region = self.reachable(self.start, None);
        if !region.contains(before.label) || !region.contains(after.label) {
            return false;
        }
        if before.label == after.label {
            return before.index < after.index;
        }
        !self
            .reachable(self.start, Some(before.label))
            .contains(after.label)
    }

    fn assert_before(&self, before: Site<'_>, after: Site<'_>, what: &str) {
        assert!(
            self.dominates(before, after),
            "{what}: {before:?} must dominate {after:?} through the cold CFG:\n{}",
            self.ir
        );
    }

    fn calls(&self, helper: &str) -> Vec<Site<'a>> {
        let needle = format!("@{helper}(");
        let mut pending: Vec<_> = self
            .sites()
            .into_iter()
            .filter(|site| site.text.contains("call ") && site.text.contains(&needle))
            .collect();
        let mut ordered = Vec::new();
        // Order by control flow, never by block emission order. Reject branches
        // whose calls do not have the fixture's expected sequential relation.
        while !pending.is_empty() {
            let first = pending
                .iter()
                .position(|candidate| {
                    pending.iter().all(|other| {
                        (candidate.label == other.label && candidate.index == other.index)
                            || self.dominates(*candidate, *other)
                    })
                })
                .unwrap_or_else(|| {
                    panic!(
                        "@{helper} calls lack a dominance order: {pending:?}\n{}",
                        self.ir
                    )
                });
            ordered.push(pending.remove(first));
        }
        ordered
    }

    // Only representation-preserving wrappers are accepted between a root
    // load and an operand, including native RS4GC's opaque identity asm.
    fn root_read(&self, operand: &str, consumer: Site<'_>) -> (&'a str, Site<'a>) {
        let (slot, read) = self.slot_read(operand, consumer);
        assert!(expression_temp_slots(self.ir, self.blocks).contains(slot),
            "cold operand must read an expression root, not its mutable source binding: {read:?}\n{}", self.ir);
        (slot, read)
    }

    fn slot_read(&self, operand: &str, consumer: Site<'_>) -> (&'a str, Site<'a>) {
        assert!(
            temp_slots::derives_from_slot_load(self.ir, operand, 16),
            "{operand} must derive from a rooted load:\n{}",
            self.ir
        );
        let mut reg = operand;
        for _ in 0..16 {
            let site = self.definition(reg);
            let def = site.text.split_once(" = ").unwrap().1;
            if def.starts_with("load ") {
                self.assert_before(site, consumer, "root reread must dominate its consumer");
                let slot = def
                    .rsplit_once(", ptr ")
                    .unwrap()
                    .1
                    .split(',')
                    .next()
                    .unwrap()
                    .trim();
                return (slot, site);
            }
            assert!(
                def.starts_with("bitcast ")
                    || def.starts_with("ptrtoint ")
                    || def.starts_with("inttoptr ")
                    || def.starts_with("and i64 ")
                    || (def.starts_with("or i64 ")
                        && def.ends_with(crate::nanbox::POINTER_TAG_I64))
                    || (def.starts_with("call i64 asm \"\"") && def.contains("\"=r,0\"")),
                "unexpected root operand transformation: {site:?}\n{}",
                self.ir
            );
            reg = registers(def).next().unwrap();
        }
        panic!("no root load for {operand}:\n{}", self.ir)
    }

    fn publications(&self, slot: &str) -> Vec<Site<'a>> {
        self.sites()
            .into_iter()
            .filter(|site| {
                site.text.starts_with("store ")
                    && !is_clear(site.text)
                    && store_parts(site.text).is_some_and(|(_, target)| target == slot)
            })
            .collect()
    }

    fn publication_before(&self, slot: &str, consumer: Site<'_>) -> Site<'a> {
        let publications: Vec<_> = self
            .publications(slot)
            .into_iter()
            .filter(|store| self.dominates(*store, consumer))
            .collect();
        assert_eq!(
            publications.len(),
            1,
            "one cold publication of {slot} must dominate {consumer:?}:\n{}",
            self.ir
        );
        publications[0]
    }
}

fn store_parts(line: &str) -> Option<(&str, &str)> {
    let rest = line.strip_prefix("store ")?;
    let (value, slot) = rest.split_once(", ptr ")?;
    Some((value, slot.split(',').next().unwrap().trim()))
}

fn is_clear(line: &str) -> bool {
    line.starts_with("store i64 0,") || line.starts_with("store ptr addrspace(1) null,")
}

fn derives_from(ir: &str, value: &str, ancestor: &str, depth: usize) -> bool {
    if value == ancestor {
        return true;
    }
    if depth == 0 {
        return false;
    }
    let needle = format!("{value} = ");
    ir.lines()
        .map(str::trim)
        .find_map(|line| line.strip_prefix(&needle))
        .is_some_and(|def| registers(def).any(|reg| derives_from(ir, reg, ancestor, depth - 1)))
}

fn expression_temp_slots(ir: &str, blocks: &BTreeMap<&str, String>) -> BTreeSet<String> {
    let (entry, _) = block(blocks, "entry.", ir);
    temp_slots::temp_root_slots(ir)
        .into_iter()
        .filter(|slot| {
            // Native roots zero-seed parameter allocas too. Exempt only roots
            // whose sole publication is an entry store of a function argument;
            // an expression root later parking that argument remains a temp.
            let stores: Vec<_> = blocks
                .iter()
                .flat_map(|(label, body)| {
                    body.lines().map(str::trim).filter_map(move |line| {
                        store_parts(line)
                            .filter(|(_, target)| *target == slot)
                            .filter(|_| !is_clear(line))
                            .map(move |(value, _)| (*label, value))
                    })
                })
                .collect();
            !(stores.len() == 1
                && stores[0].0 == entry
                && registers(ir.lines().next().unwrap()).any(|argument| {
                    registers(stores[0].1).any(|reg| derives_from(ir, reg, argument, 8))
                }))
        })
        .collect()
}

fn assert_hot_has_no_temp_traffic(
    ir: &str,
    blocks: &BTreeMap<&str, String>,
    start: &str,
    stop: &str,
) {
    let cfg = ColdCfg {
        ir,
        blocks,
        start,
        stop,
    };
    let slots = expression_temp_slots(ir, blocks);
    for label in cfg.reachable(start, None) {
        let body = &blocks[label];
        for token in registers(body) {
            assert!(
                !slots.contains(token),
                "hot block {label} touches temp root {token}:\n{ir}"
            );
        }
        assert!(
            !body.contains("js_gc_temp_root_"),
            "hot block has runtime root traffic:\n{ir}"
        );
    }
}

fn field_store_ir(field_ty: Type) -> String {
    // A declared number alone has an Any birth lane. Use an actual early
    // numeric initializer so this fixture reaches the raw-f64 store arm.
    let init = (field_ty == Type::Number).then_some(Expr::Number(0.0));
    let mut module = Module::new("collecting_field_store.ts");
    module.classes = vec![Class {
        id: 101,
        name: "Boxed".to_string(),
        type_params: Vec::new(),
        extends: None,
        extends_name: None,
        native_extends: None,
        extends_expr: None,
        heritage_lexically_shadowed: false,
        fields: vec![ClassField {
            name: "v".to_string(),
            key_expr: None,
            ty: field_ty,
            init,
            is_private: false,
            is_readonly: false,
            decorators: Vec::new(),
        }],
        constructor: None,
        methods: Vec::new(),
        getters: Vec::new(),
        setters: Vec::new(),
        static_accessor_names: Vec::new(),
        static_accessor_fn_ids: Vec::new(),
        computed_members: Vec::new(),
        static_fields: Vec::new(),
        static_methods: Vec::new(),
        decorators: Vec::new(),
        is_exported: false,
        aliases: Vec::new(),
        is_nested: false,
        alloc_width_hint: 0,
        specialized_from: None,
    }];
    ir_for(
        module,
        probe(
            vec![Type::Named("Boxed".to_string()), Type::Any],
            Expr::PropertySet {
                object: Box::new(Expr::LocalGet(1)),
                property: "v".to_string(),
                value: Box::new(Expr::LocalGet(2)),
            },
        ),
    )
}

#[test]
fn collecting_field_guard_refreshes_store_fallback_and_assignment_only_on_cold_paths() {
    crate::temp_root_coverage::under_both_lowerings(|mode| {
        for field_ty in [Type::Any, Type::Number] {
            let numeric = field_ty == Type::Number;
            let ir = field_store_ir(field_ty);
            let blocks = blocks(&ir);
            let (hot_label, _) = block(&blocks, "class_field_set.fast.", &ir);
            let (merge_label, merge) = block(&blocks, "class_field_set.merge.", &ir);
            let (guard_entry, _) = block(&blocks, "class_field_inline.guardcall.", &ir);
            let (cold_label, _) = block(&blocks, "class_field_set.cold_fast.", &ir);
            let (cold_merge_label, _) = block(&blocks, "class_field_set.cold_merge.", &ir);
            let cfg = ColdCfg {
                ir: &ir,
                blocks: &blocks,
                start: guard_entry,
                stop: merge_label,
            };
            let guards = cfg.calls("js_typed_feedback_class_field_set_guard");
            assert_eq!(guards.len(), 1, "{mode}: one collecting guard:\n{ir}");
            let guard = guards[0];
            let guard_body = &blocks[guard.label];
            assert!(
                successors(guard_body).any(|label| label == cold_label),
                "{mode}: collecting guard must enter rooted cold store:\n{ir}"
            );
            assert!(
                !cfg.reachable(guard_entry, None).contains(hot_label),
                "collecting edge must not enter unrooted hot store:\n{ir}"
            );
            assert!(
                blocks.values().any(|body| body.contains("br i1 ")
                    && successors(body).any(|label| label == hot_label)),
                "inline store must remain reachable:\n{ir}"
            );
            assert_hot_has_no_temp_traffic(&ir, &blocks, hot_label, merge_label);
            let guard_args =
                temp_slots::call_operands(guard.text, "js_typed_feedback_class_field_set_guard")
                    .unwrap();
            let (receiver_slot, _) = cfg.root_read(&guard_args[1], guard);
            let (rhs_slot, _) = cfg.root_read(&guard_args[6], guard);
            assert_ne!(
                receiver_slot, rhs_slot,
                "receiver and RHS need distinct live roots:\n{ir}"
            );
            for slot in [receiver_slot, rhs_slot] {
                cfg.publication_before(slot, guard);
            }

            // Inspect every store reachable from the guard-pass entry until
            // its cold merge. Number fields legitimately store either the
            // reread or the NaN canonicalizer's result.
            let store_cfg = ColdCfg {
                ir: &ir,
                blocks: &blocks,
                start: cold_label,
                stop: cold_merge_label,
            };
            let stores: Vec<_> = store_cfg
                .sites()
                .into_iter()
                .filter(|site| site.text.starts_with("store double "))
                .collect();
            assert!(
                !stores.is_empty(),
                "cold guard-pass arm must store a value:\n{ir}"
            );
            let mut canonical_stores = 0;
            for store in stores {
                let (value, target) = store_parts(store.text).unwrap();
                let stored = value.strip_prefix("double ").unwrap();
                let definition = cfg.definition(stored);
                let rhs = if definition
                    .text
                    .contains("call double @js_array_numeric_value_to_raw_f64(")
                {
                    assert!(numeric, "only Number fields canonicalize raw f64:\n{ir}");
                    canonical_stores += 1;
                    cfg.assert_before(definition, store, "canonicalizer result must reach store");
                    temp_slots::call_operands(definition.text, "js_array_numeric_value_to_raw_f64")
                        .unwrap()[0]
                        .clone()
                } else {
                    stored.to_string()
                };
                let (slot, reload) = cfg.root_read(&rhs, store);
                assert_eq!(
                    slot, rhs_slot,
                    "guard-pass store must consume refreshed RHS:\n{ir}"
                );
                cfg.assert_before(
                    guard,
                    reload,
                    "store RHS must be reread after collecting guard",
                );
                let receiver_load = cfg.sites().into_iter().find(|site| {
                    site.text.contains(" = load ")
                        && site.text.rsplit_once(", ptr ").is_some_and(|(_, tail)| tail.split(',').next().unwrap().trim() == receiver_slot)
                        && cfg.dominates(guard, *site) && cfg.dominates(*site, store)
                        && derives_from(&ir, target, site.text.split_once(" = ").unwrap().0, 16)
                }).unwrap_or_else(|| panic!("store address must derive from post-guard receiver reread: {store:?}\n{ir}"));
                cfg.assert_before(guard, receiver_load, "receiver must refresh after guard");
            }
            if numeric {
                assert!(
                    canonical_stores > 0,
                    "Number cold path must retain non-finite/boxed-number canonicalization:\n{ir}"
                );
            }

            let fallbacks = cfg.calls("js_class_field_set_fallback");
            assert_eq!(fallbacks.len(), 1, "one setter fallback:\n{ir}");
            let fallback = fallbacks[0];
            let fallback_args =
                temp_slots::call_operands(fallback.text, "js_class_field_set_fallback").unwrap();
            for (operand, expected) in [
                (&fallback_args[1], receiver_slot),
                (&fallback_args[3], rhs_slot),
            ] {
                let (slot, reload) = cfg.root_read(operand, fallback);
                assert_eq!(
                    slot, expected,
                    "fallback must read same captured root:\n{ir}"
                );
                cfg.assert_before(
                    guard,
                    reload,
                    "fallback reread must follow collecting guard",
                );
            }
            assert!(
                blocks[fallback.label].contains("load double, ptr @"),
                "fallback must reload immutable key handle:\n{ir}"
            );

            let phi = merge
                .lines()
                .find(|line| line.contains("phi double"))
                .unwrap();
            let incoming: Vec<_> = phi
                .split('[')
                .skip(1)
                .map(|tail| {
                    let (value, predecessor) = tail.split_once(',').unwrap();
                    (
                        value.trim(),
                        predecessor
                            .split(']')
                            .next()
                            .unwrap()
                            .trim()
                            .trim_start_matches('%'),
                    )
                })
                .filter(|(_, predecessor)| cfg.reachable(guard_entry, None).contains(predecessor))
                .collect();
            assert_eq!(
                incoming.len(),
                1,
                "assignment has one cold result edge:\n{ir}"
            );
            let (cold_result, cold_end) = incoming[0];
            let result_use = Site {
                label: cold_end,
                index: blocks[cold_end].lines().count(),
                text: "cold result edge",
            };
            let (slot, result_read) = cfg.root_read(cold_result, result_use);
            assert_eq!(slot, rhs_slot, "assignment must return captured RHS:\n{ir}");
            // The fallback need not dominate the join: both alternatives enter
            // it. Require the join to be reachable from each and the reload
            // to reside there, after a possible setter collection.
            assert_eq!(
                result_read.label, cold_merge_label,
                "assignment reread belongs in cold merge:\n{ir}"
            );
            let fallback_cfg = ColdCfg {
                ir: &ir,
                blocks: &blocks,
                start: fallback.label,
                stop: merge_label,
            };
            fallback_cfg.assert_before(
                fallback,
                result_read,
                "assignment must reread after the possible setter collection",
            );
            for slot in [receiver_slot, rhs_slot] {
                let clears: Vec<_> = cfg
                    .sites()
                    .into_iter()
                    .filter(|site| {
                        is_clear(site.text)
                            && store_parts(site.text).is_some_and(|(_, target)| target == slot)
                    })
                    .collect();
                assert_eq!(
                    clears.len(),
                    1,
                    "cold group must release {slot} exactly once:\n{ir}"
                );
                cfg.assert_before(
                    result_read,
                    clears[0],
                    "assignment reread must precede root release",
                );
                cfg.assert_before(
                    clears[0],
                    result_use,
                    "release must precede cold result edge",
                );
            }
        }
    });
}

fn add(left: Expr, right: Expr) -> Expr {
    Expr::Binary {
        op: BinaryOp::Add,
        left: Box::new(left),
        right: Box::new(right),
    }
}

const ADD_HELPER: &str = "js_dynamic_string_or_number_add";

#[test]
fn single_dynamic_add_has_no_prior_call_window_or_added_temp_roots() {
    crate::temp_root_coverage::under_both_lowerings(|mode| {
        let ir = ir_for(
            Module::new("collecting_single_add.ts"),
            probe(
                vec![Type::Any; 2],
                add(Expr::LocalGet(1), Expr::LocalGet(2)),
            ),
        );
        let blocks = blocks(&ir);
        let (slow, _) = block(&blocks, "guarded_add.dynamic.", &ir);
        let (merge, _) = block(&blocks, "guarded_add.merge.", &ir);
        let cfg = ColdCfg {
            ir: &ir,
            blocks: &blocks,
            start: slow,
            stop: merge,
        };
        assert_eq!(
            cfg.calls(ADD_HELPER).len(),
            1,
            "{mode}: one consuming add:\n{ir}"
        );
        assert_hot_has_no_temp_traffic(&ir, &blocks, slow, merge);
        assert!(expression_temp_slots(&ir, &blocks).is_empty(),
            "single consuming add has no earlier collection window or expression-root allocations:\n{ir}");
    });
}

#[test]
fn right_nested_add_preserves_captured_leaf_across_inner_coercion() {
    crate::temp_root_coverage::under_both_lowerings(|mode| {
        let ir = ir_for(
            Module::new("collecting_nested_add.ts"),
            probe(
                vec![Type::Any; 3],
                add(Expr::LocalGet(1), add(Expr::LocalGet(2), Expr::LocalGet(3))),
            ),
        );
        let blocks = blocks(&ir);
        let (fast_label, fast) = block(&blocks, "guarded_add.numeric.", &ir);
        let (merge_label, _) = block(&blocks, "guarded_add.merge.", &ir);
        assert_eq!(
            fast.matches("fadd double").count(),
            2,
            "{mode}: numeric tree:\n{ir}"
        );
        assert_hot_has_no_temp_traffic(&ir, &blocks, fast_label, merge_label);
        let (slow, _) = block(&blocks, "guarded_add.dynamic.", &ir);
        let cfg = ColdCfg {
            ir: &ir,
            blocks: &blocks,
            start: slow,
            stop: merge_label,
        };
        let calls = cfg.calls(ADD_HELPER);
        assert_eq!(
            calls.len(),
            2,
            "{mode}: inner and outer consuming calls:\n{ir}"
        );
        let outer = temp_slots::call_operands(calls[1].text, ADD_HELPER).unwrap();
        let (slot, reread) = cfg.root_read(&outer[0], calls[1]);
        cfg.publication_before(slot, calls[0]);
        cfg.assert_before(
            calls[0],
            reread,
            "saved leaf must be reread after inner coercion",
        );
        let inner_result = calls[0].text.split_once(" = ").unwrap().0;
        assert!(
            derives_from(&ir, &outer[1], inner_result, 16),
            "outer right must consume inner result:\n{ir}"
        );
    });
}

#[test]
fn balanced_add_roots_left_intermediate_across_right_subtree() {
    crate::temp_root_coverage::under_both_lowerings(|mode| {
        let ir = ir_for(
            Module::new("collecting_balanced_add.ts"),
            probe(
                vec![Type::Any; 4],
                add(
                    add(Expr::LocalGet(1), Expr::LocalGet(2)),
                    add(Expr::LocalGet(3), Expr::LocalGet(4)),
                ),
            ),
        );
        let blocks = blocks(&ir);
        let (fast, fast_body) = block(&blocks, "guarded_add.numeric.", &ir);
        let (merge, _) = block(&blocks, "guarded_add.merge.", &ir);
        assert_eq!(
            fast_body.matches("fadd double").count(),
            3,
            "{mode}: numeric tree:\n{ir}"
        );
        assert_hot_has_no_temp_traffic(&ir, &blocks, fast, merge);
        let (slow, _) = block(&blocks, "guarded_add.dynamic.", &ir);
        let cfg = ColdCfg {
            ir: &ir,
            blocks: &blocks,
            start: slow,
            stop: merge,
        };
        let calls = cfg.calls(ADD_HELPER);
        assert_eq!(calls.len(), 3, "{mode}: left, right and outer calls:\n{ir}");
        let intermediate = calls[0].text.split_once(" = ").unwrap().0;
        let slot = temp_slots::temp_root_slot_holding(&ir, intermediate)
            .unwrap_or_else(|| panic!("left intermediate must enter an expression root:\n{ir}"));
        let publication = cfg.publication_before(&slot, calls[1]);
        cfg.assert_before(
            calls[0],
            publication,
            "left intermediate publication must follow its producer",
        );
        let (stored, _) = store_parts(publication.text).unwrap();
        assert!(
            registers(stored).any(|reg| derives_from(&ir, reg, intermediate, 16)),
            "root dominating right call must hold left intermediate:\n{ir}"
        );
        let outer = temp_slots::call_operands(calls[2].text, ADD_HELPER).unwrap();
        let (reread_slot, reread) = cfg.root_read(&outer[0], calls[2]);
        assert_eq!(
            reread_slot, slot,
            "outer left must read intermediate root:\n{ir}"
        );
        cfg.assert_before(
            calls[1],
            reread,
            "left intermediate reread must follow right subtree collection",
        );
        let right_result = calls[1].text.split_once(" = ").unwrap().0;
        assert!(
            derives_from(&ir, &outer[1], right_result, 16),
            "outer right must consume right-subtree result:\n{ir}"
        );
    });
}

/// `backstop == false` compiles without `root_reload`, so the IR is what the
/// `new` lowering itself emits rather than what the whole-function reload pass
/// repairs afterwards.
fn imported_constructor_ir(
    walk_ancestor: bool,
    has_rest: bool,
    has_arguments: bool,
    backstop: bool,
) -> String {
    let mut opts = crate::temp_root_coverage::entry_opts();
    opts.imported_classes.push(crate::ImportedClass {
        name: "WorkerMade".to_string(),
        local_alias: None,
        namespace: None,
        source_prefix: "worker_producer_ts".to_string(),
        // One positional parameter ahead of any packed slot, so every
        // configuration dispatches a positional operand as well as arrays.
        constructor_param_count: 1 + usize::from(has_rest) + usize::from(has_arguments),
        has_own_constructor: true,
        constructor_has_rest: has_rest,
        constructor_has_synthetic_arguments: has_arguments,
        has_instance_fields: true,
        method_names: Vec::new(),
        proven_this_method_names: Vec::new(),
        proven_this_tower_method_names: Vec::new(),
        method_return_types: Vec::new(),
        method_param_counts: Vec::new(),
        method_has_rest: Vec::new(),
        method_has_synthetic_arguments: Vec::new(),
        method_arguments_length_only: Vec::new(),
        static_field_names: Vec::new(),
        static_method_names: Vec::new(),
        static_method_return_types: Vec::new(),
        static_method_param_counts: Vec::new(),
        static_method_has_rest: Vec::new(),
        static_method_has_user_rest: Vec::new(),
        static_method_has_synthetic_arguments: Vec::new(),
        getter_names: Vec::new(),
        getter_return_types: Vec::new(),
        setter_names: Vec::new(),
        parent_name: None,
        field_names: vec!["v".to_string(), "w".to_string()],
        field_types: vec![Type::Any, Type::Any],
        source_class_id: Some(101),
        return_shape_imports: Vec::new(),
        object_literal: None,
    });
    let mut module = Module::new("collecting_imported_constructor.ts");
    if walk_ancestor {
        module.classes.push(Class {
            id: 102,
            name: "Leaf".to_string(),
            type_params: Vec::new(),
            extends: Some(101),
            extends_name: Some("WorkerMade".to_string()),
            native_extends: None,
            extends_expr: None,
            heritage_lexically_shadowed: false,
            fields: Vec::new(),
            constructor: None,
            methods: Vec::new(),
            getters: Vec::new(),
            setters: Vec::new(),
            static_accessor_names: Vec::new(),
            static_accessor_fn_ids: Vec::new(),
            computed_members: Vec::new(),
            static_fields: Vec::new(),
            static_methods: Vec::new(),
            decorators: Vec::new(),
            is_exported: false,
            aliases: Vec::new(),
            is_nested: false,
            alloc_width_hint: 0,
            specialized_from: None,
        });
    }
    module.functions.push(probe(
        vec![Type::Any],
        Expr::New {
            class_name: if walk_ancestor { "Leaf" } else { "WorkerMade" }.to_string(),
            // A real pointer-bearing argument, not an undefined padding value.
            args: vec![Expr::LocalGet(1), Expr::Object(Vec::new())],
            type_args: Vec::new(),
            cap_args_appended: 0,
            byte_offset: 0,
        },
    ));
    let symbol = user_function_symbol(&module.name, "probe");
    let previous = crate::root_reload::TEST_SKIP_ROOT_RELOAD.with(|skip| skip.replace(!backstop));
    let compiled = compile_module(&module, opts);
    crate::root_reload::TEST_SKIP_ROOT_RELOAD.with(|skip| skip.set(previous));
    let ir = String::from_utf8(compiled.expect("imported ctor fixture compiles")).unwrap();
    function_slice(&ir, &symbol).to_string()
}

#[test]
fn imported_constructor_receiver_refreshes_after_initializers_and_call_preparation() {
    crate::temp_root_coverage::under_both_lowerings(|mode| {
        for (walk_ancestor, backstop) in
            [(false, true), (true, true), (false, false), (true, false)]
        {
            for (has_rest, has_arguments) in
                [(false, false), (true, false), (false, true), (true, true)]
            {
                let packed = has_rest || has_arguments;
                let ir = imported_constructor_ir(walk_ancestor, has_rest, has_arguments, backstop);
                let blocks = blocks(&ir);
                let (entry, _) = block(&blocks, "entry.", &ir);
                let cfg = ColdCfg {
                    ir: &ir,
                    blocks: &blocks,
                    start: entry,
                    stop: "__end_of_function__",
                };
                let ctor = "worker_producer_ts__WorkerMade_constructor";
                let calls = cfg.calls(ctor);
                assert_eq!(calls.len(), 1, "{mode}: one imported ctor dispatch:\n{ir}");
                let call = calls[0];
                let operands = temp_slots::call_operands(call.text, ctor).unwrap();
                let (this_slot, read) = cfg.slot_read(&operands[0], call);
                // The positional argument is re-read from its root at
                // dispatch; packed arrays must each own a distinct expression
                // root through dispatch.
                let fixed_arg = Some(cfg.slot_read(&operands[1], call));
                let packed_reads: Vec<_> = if packed {
                    operands[2..]
                        .iter()
                        .map(|arg| cfg.root_read(arg, call))
                        .collect()
                } else {
                    Vec::new()
                };
                assert_eq!(
                    packed_reads.len(),
                    usize::from(has_rest) + usize::from(has_arguments)
                );
                if packed_reads.len() == 2 {
                    assert_ne!(
                        packed_reads[0].0, packed_reads[1].0,
                        "rest and arguments must retain separate arrays:\n{ir}"
                    );
                }
                if mode == "native roots" {
                    assert!(
                        cfg.definition(this_slot)
                            .text
                            .contains("alloca ptr addrspace(1)"),
                        "constructor this-slot must be a native GC root:\n{ir}"
                    );
                } else {
                    assert!(
                        crate::testing::root_slots::bound_slots(&ir).contains_key(this_slot),
                        "constructor this-slot must be bound in the shadow frame:\n{ir}"
                    );
                }
                let allocation = cfg
                    .sites()
                    .into_iter()
                    .find(|site| site.text.contains(" = call i64 @js_object_alloc_class_"))
                    .expect("fixture must allocate a class instance");
                let allocated = allocation.text.split_once(" = ").unwrap().0;
                let publications: Vec<_> = cfg
                    .publications(this_slot)
                    .into_iter()
                    .filter(|site| {
                        registers(store_parts(site.text).unwrap().0)
                            .any(|value| derives_from(&ir, value, allocated, 16))
                    })
                    .collect();
                assert_eq!(
                    publications.len(),
                    1,
                    "allocation must publish into this root:\n{ir}"
                );
                let publication = publications[0];
                cfg.assert_before(
                    allocation,
                    publication,
                    "root publication follows allocation",
                );
                cfg.assert_before(publication, call, "root publication dominates constructor");
                for helper in [
                    "js_class_value",
                    "js_typed_feedback_class_field_set_guard",
                    "js_class_field_set_fallback",
                    "js_array_alloc",
                ] {
                    let sites: Vec<_> = cfg
                        .sites()
                        .into_iter()
                        .filter(|site| site.text.contains(&format!("@{helper}(")))
                        .filter(|site| {
                            let path = ColdCfg {
                                start: site.label,
                                ..cfg
                            };
                            path.reachable(site.label, None).contains(call.label)
                        })
                        .collect();
                    if helper != "js_array_alloc" || packed {
                        assert!(
                            !sites.is_empty(),
                            "{mode}: collecting subject {helper} must be live:\n{ir}"
                        );
                    }
                    for site in sites {
                        cfg.assert_before(
                            publication,
                            site,
                            "this root precedes collecting preparation",
                        );
                        let path = ColdCfg {
                            start: site.label,
                            ..cfg
                        };
                        path.assert_before(
                            site,
                            read,
                            "receiver reread follows collecting preparation",
                        );
                        if let Some((_, arg_read)) = fixed_arg {
                            path.assert_before(
                                site,
                                arg_read,
                                "fixed argument reread follows collecting preparation",
                            );
                        }
                    }
                }
                // Every value pushed into a packed array, and the positional
                // operand at dispatch, is re-read from its root below every
                // collecting call that precedes its use: the array allocation,
                // each earlier push, and the class-value lookup.
                let collecting = |site: &Site<'_>| {
                    [
                        "js_array_alloc",
                        "js_array_push_f64",
                        "js_array_push_f64_temp_rooted",
                        "js_class_value",
                    ]
                    .iter()
                    .any(|helper| site.text.contains(&format!("@{helper}(")))
                };
                let pushes: Vec<_> = cfg
                    .sites()
                    .into_iter()
                    .filter_map(|site| {
                        ["js_array_push_f64", "js_array_push_f64_temp_rooted"]
                            .into_iter()
                            .find(|helper| site.text.contains(&format!("@{helper}(")))
                            .map(|helper| (site, helper))
                    })
                    .filter(|(site, _)| cfg.dominates(*site, call))
                    .collect();
                let expected_pushes = usize::from(has_rest) + 2 * usize::from(has_arguments);
                assert_eq!(
                    pushes.len(),
                    expected_pushes,
                    "{mode}: one push per packed element:\n{ir}"
                );
                let mut checked: Vec<(Site<'_>, Site<'_>)> = pushes
                    .iter()
                    .map(|(push, helper)| {
                        let pushed = temp_slots::call_operands(push.text, helper).unwrap();
                        (*push, cfg.slot_read(&pushed[1], *push).1)
                    })
                    .collect();
                if let Some((_, arg_read)) = fixed_arg {
                    checked.push((call, arg_read));
                }
                for (consumer, value_read) in checked {
                    for site in cfg.sites().into_iter().filter(|site| {
                        collecting(site)
                            && !(site.label == consumer.label && site.index == consumer.index)
                            && cfg.dominates(*site, consumer)
                    }) {
                        cfg.assert_before(
                            site,
                            value_read,
                            "argument reread follows every collecting call before its use",
                        );
                    }
                }
                for (slot, packed_read) in packed_reads {
                    let publications = cfg.publications(slot);
                    // The pool can reuse a released field-initializer root.
                    // Identify the array's publication by its allocation;
                    // earlier uses of the same slot are separate lifetimes.
                    let initial: Vec<_> = cfg
                        .sites()
                        .into_iter()
                        .filter(|site| site.text.contains(" = call i64 @js_array_alloc("))
                        .flat_map(|producer| {
                            publications.iter().copied().filter_map(move |publication| {
                                registers(store_parts(publication.text).unwrap().0)
                                    .any(|value| {
                                        derives_from(
                                            cfg.ir,
                                            value,
                                            producer.text.split_once(" = ").unwrap().0,
                                            16,
                                        )
                                    })
                                    .then_some((producer, publication))
                            })
                        })
                        .collect();
                    assert_eq!(initial.len(), 1,
                        "{mode}: ancestor={walk_ancestor}, rest={has_rest}, arguments={has_arguments}: one initial packed-array publication in {slot}; publications={publications:?}\n{ir}");
                    let (producer, first) = initial[0];
                    for update in publications.iter().copied().filter(|site| {
                        cfg.dominates(producer, *site)
                            && !(site.label == first.label && site.index == first.index)
                    }) {
                        cfg.assert_before(first, update, "array publication dominates its updates");
                    }
                    cfg.assert_before(
                        producer,
                        first,
                        "packed allocation precedes root publication",
                    );
                    cfg.assert_before(first, call, "packed root survives through dispatch");
                    for helper in ["js_array_alloc", "js_array_push_f64", "js_class_value"] {
                        for site in cfg.sites().into_iter().filter(|site| {
                            site.text.contains(&format!("@{helper}("))
                                && cfg.dominates(producer, *site)
                        }) {
                            cfg.assert_before(
                                first,
                                site,
                                "packed root precedes subsequent collecting calls",
                            );
                            let path = ColdCfg {
                                start: site.label,
                                ..cfg
                            };
                            path.assert_before(
                                site,
                                packed_read,
                                "packed argument reread follows collecting preparation",
                            );
                        }
                    }
                    let clears: Vec<_> = cfg
                        .sites()
                        .into_iter()
                        .filter(|site| {
                            is_clear(site.text)
                                && cfg.dominates(call, *site)
                                && store_parts(site.text).is_some_and(|(_, target)| target == slot)
                        })
                        .collect();
                    assert_eq!(clears.len(), 1, "packed root releases once:\n{ir}");
                    cfg.assert_before(
                        call,
                        clears[0],
                        "packed root releases after constructor dispatch",
                    );
                }
                // Existing field-store hot blocks keep their direct stores;
                // this repair adds no root publication or reread in those arms.
                for (label, body) in &blocks {
                    if label.starts_with("class_field_set.fast.") {
                        let stop = successors(body).next().expect("hot store must rejoin");
                        assert_hot_has_no_temp_traffic(&ir, &blocks, label, stop);
                    }
                }
            }
        }
    });
}
