//! Whole-module inline-birth invariant: follow actual SSA header operands and
//! every CFG path back from each seed, rather than counting names in one function.
use std::collections::{BTreeMap, BTreeSet};

fn store_target(line: &str) -> Option<&str> {
    line.strip_prefix("store ")?
        .split_once(", ptr ")
        .map(|(_, target)| target.split(',').next().unwrap().trim())
}

/// Sabotage the actual emitted header's SSA dependency, separately for each
/// allocator. Keep the packed type/size word and the seed unchanged: only the
/// live color is dropped. Also move a real seed immediately after its header.
pub(super) fn sabotage_controls(ir: &str) {
    for vector in [false, true] {
        let mut changed = false;
        let mut pieces = ir
            .split("\ndefine ")
            .map(str::to_string)
            .collect::<Vec<_>>();
        for function in pieces.iter_mut().skip(1) {
            let lines = function.lines().map(str::trim).collect::<Vec<_>>();
            let defs: BTreeMap<_, _> = lines
                .iter()
                .filter_map(|line| line.split_once(" = "))
                .map(|(lhs, rhs)| (lhs.to_string(), rhs.to_string()))
                .collect();
            let header = lines.iter().find(|line| {
                line.starts_with(if vector {
                    "store <2 x i64> %"
                } else {
                    "store i64 %"
                }) && registers(line).first().is_some_and(|value| {
                    let mut deps = BTreeSet::new();
                    dependencies(value, &defs, &mut deps);
                    deps.iter()
                        .filter_map(|r| defs.get(r))
                        .any(|rhs| rhs.starts_with("load volatile i8"))
                })
            });
            let Some(header) = header else { continue };
            let mut deps = BTreeSet::new();
            dependencies(&registers(header)[0], &defs, &mut deps);
            let (name, rhs) = defs
                .iter()
                .find(|(name, rhs)| {
                    deps.contains(*name) && rhs.starts_with("shl i64 ") && rhs.ends_with(", 8")
                })
                .unwrap();
            *function = function.replacen(
                &format!("{name} = {rhs}"),
                &format!("{name} = shl i64 0, 8"),
                1,
            );
            changed = true;
            break;
        }
        assert!(changed, "both allocator sabotage subjects must be live");
        assert!(
            std::panic::catch_unwind(|| check(&pieces.join("\ndefine "))).is_err(),
            "missing header birth flags must fail for vector={vector}"
        );
    }
    let mut pieces = ir
        .split("\ndefine ")
        .map(str::to_string)
        .collect::<Vec<_>>();
    let mut changed = false;
    for function in pieces.iter_mut().skip(1) {
        let mut lines = function.lines().map(str::to_string).collect::<Vec<_>>();
        let Some(seed) = lines
            .iter()
            .position(|l| l.contains("call ") && l.contains("@js_gc_note_black_birth("))
        else {
            continue;
        };
        let call = lines.remove(seed);
        let raw = call
            .split("@js_gc_note_black_birth(ptr ")
            .nth(1)
            .unwrap()
            .split(',')
            .next()
            .unwrap();
        let header = lines
            .iter()
            .position(|l| store_target(l.trim()) == Some(raw))
            .unwrap();
        lines.insert(header + 1, call);
        *function = lines.join("\n");
        changed = true;
        break;
    }
    assert!(changed);
    assert!(
        std::panic::catch_unwind(|| check(&pieces.join("\ndefine "))).is_err(),
        "seed before initialization must fail"
    );
}

fn registers(text: &str) -> Vec<String> {
    text.split('%')
        .skip(1)
        .map(|s| {
            format!(
                "%{}",
                s.chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '.')
                    .collect::<String>()
            )
        })
        .collect()
}

fn dependencies(value: &str, defs: &BTreeMap<String, String>, seen: &mut BTreeSet<String>) {
    if !seen.insert(value.to_string()) {
        return;
    }
    if let Some(rhs) = defs.get(value) {
        for child in registers(rhs) {
            dependencies(&child, defs, seen);
        }
    }
}

pub(super) fn check(ir: &str) {
    let mut sites = 0;
    let mut arrays = 0;
    let mut objects = 0;
    for function in ir.split("\ndefine ").skip(1) {
        let lines: Vec<_> = function
            .split_once("\n}")
            .unwrap()
            .0
            .lines()
            .map(str::trim)
            .collect();
        let defs: BTreeMap<_, _> = lines
            .iter()
            .filter_map(|line| line.split_once(" = "))
            .map(|(lhs, rhs)| (lhs.to_string(), rhs.to_string()))
            .collect();
        let mut blocks = BTreeMap::<String, (usize, usize)>::new();
        let mut current = "entry".to_string();
        let mut begin = 0;
        for (i, line) in lines.iter().enumerate() {
            if let Some(label) = line.strip_suffix(':') {
                blocks.insert(current, (begin, i));
                current = label.to_string();
                begin = i + 1;
            }
        }
        blocks.insert(current, (begin, lines.len()));
        let mut predecessors = BTreeMap::<String, Vec<String>>::new();
        for (label, &(start, end)) in &blocks {
            for line in &lines[start..end] {
                if line.starts_with("br ") {
                    for target in line.split("label %").skip(1) {
                        let name = target.split([',', ' ']).next().unwrap().to_string();
                        predecessors.entry(name).or_default().push(label.clone());
                    }
                }
            }
        }
        for slow in lines.iter().filter(|l| l.contains(super::INLINE_SLOW_CALL)) {
            sites += 1;
            let slow_reg = slow.split_once(" = ").unwrap().0;
            let (raw, _) = defs
                .iter()
                .find(|(_, rhs)| {
                    rhs.starts_with("phi ptr ") && registers(rhs).iter().any(|r| r == slow_reg)
                })
                .expect("raw fast/slow merge");
            let (header_at, header) = lines
                .iter()
                .enumerate()
                .find(|(_, l)| store_target(l) == Some(raw.as_str()))
                .expect("each raw allocation must write a header");
            let operand = registers(header)
                .into_iter()
                .next()
                .expect("header must carry live flags, not a constant");
            let mut deps = BTreeSet::new();
            dependencies(&operand, &defs, &mut deps);
            let (flags_reg, _) = defs
                .iter()
                .find(|(name, rhs)| {
                    deps.contains(*name) && rhs.starts_with("load volatile i8, ptr ")
                })
                .expect("header must depend on a LIVE birth flag load");
            let address_reg = registers(&defs[flags_reg])[0].clone();
            let field_reg = registers(&defs[&address_reg])[0].clone();
            assert!(
                defs[&field_reg].starts_with("getelementptr i8, ptr ")
                    && defs[&field_reg].ends_with(", i64 24"),
                "must read InlineArenaState's runtime birth cell address"
            );
            let packed = deps
                .iter()
                .filter_map(|r| defs.get(r))
                .find(|rhs| rhs.starts_with("or i64 "))
                .expect("birth flags must be ORed in the header store");
            assert!(
                deps.iter()
                    .filter_map(|r| defs.get(r))
                    .any(|rhs| rhs.starts_with("shl i64 ") && rhs.ends_with(", 8")),
                "birth flags occupy GcHeader byte 1"
            );
            let constant: u64 = std::iter::once(packed)
                .chain(deps.iter().filter_map(|r| defs.get(r)))
                .flat_map(|rhs| rhs.split_whitespace())
                .filter_map(|s| s.trim_matches([',', '>']).parse::<u64>().ok())
                .find(|word| (16..=4096).contains(&(word >> 32)))
                .expect("packed header size");
            let base = if header.starts_with("store <2 x i64>") {
                objects += 1;
                24
            } else {
                arrays += 1;
                16
            };
            let total = (constant >> 32) as usize;
            let slots = (total - base) / 8;
            let seed_needle = format!("@js_gc_note_black_birth(ptr {raw}, ptr ");
            let seed_at = lines
                .iter()
                .position(|l| l.contains(&seed_needle))
                .unwrap_or_else(|| {
                    panic!(
                        "every non-leaf inline birth must seed {raw}; calls: {:?}",
                        lines
                            .iter()
                            .filter(|l| l.contains("js_gc_note_black_birth"))
                            .collect::<Vec<_>>()
                    )
                });
            assert!(seed_at > header_at, "seed cannot precede header");
            let seed_args = registers(lines[seed_at]);
            assert_eq!(seed_args.len(), 2, "seed must receive its resolved queue");
            assert_eq!(&seed_args[0], raw);
            let queue_load = &defs[&seed_args[1]];
            assert!(queue_load.starts_with("load ptr, ptr "));
            let queue_field = &defs[&registers(queue_load)[0]];
            assert!(queue_field.starts_with("getelementptr i8, ptr "));
            assert!(queue_field.ends_with(", i64 32"));
            let queue_state = registers(queue_field)[0].clone();
            assert!(
                deps.iter().any(|dep| defs.get(dep).is_some_and(|rhs| {
                    rhs.starts_with("getelementptr i8, ptr ")
                        && rhs.ends_with(", i64 24")
                        && registers(rhs).first() == Some(&queue_state)
                })),
                "flags and seed queue must come from the same thread's inline state"
            );
            let (seed_block, _) = blocks
                .iter()
                .find(|(_, (start, end))| *start <= seed_at && seed_at < *end)
                .unwrap();
            let mut pending = vec![(
                seed_block.clone(),
                seed_at,
                BTreeSet::new(),
                BTreeSet::new(),
            )];
            while let Some((block, end, mut initialized, mut visited)) = pending.pop() {
                assert!(
                    visited.insert(block.clone()),
                    "unexpected cycle before birth publication"
                );
                let start = blocks[&block].0;
                for &line in lines[start..end].iter().rev() {
                    if line.contains("call ") && line.contains(" asm ") {
                        assert!(
                            line.contains("asm \"\", \"=r,0\"")
                                && line.contains("\"gc-leaf-function\""),
                            "only the non-collecting RS4GC root-reload launder is allowed: {line}"
                        );
                    } else if line.contains("call ") {
                        let callee = line.split('@').nth(1).unwrap().split('(').next().unwrap();
                        let effects = include_str!("../gc_effects/linux-x86_64.tsv");
                        assert!(
                            effects
                                .lines()
                                .any(|effect| effect == format!("{callee}\tLeaf")),
                            "collecting call before completed birth: {line}"
                        );
                    }
                    if line.starts_with("store ") {
                        if let Some(value) = registers(line.split(", ptr ").next().unwrap()).first()
                        {
                            let mut stored_deps = BTreeSet::new();
                            dependencies(value, &defs, &mut stored_deps);
                            assert!(
                                !stored_deps.contains(raw),
                                "birth published before seed: {line}"
                            );
                        }
                        if let Some(ptr) = line.split(", ptr ").nth(1) {
                            if let Some(gep) = defs.get(ptr.split(',').next().unwrap()) {
                                if gep.contains(&format!(", ptr {raw}, i64 ")) {
                                    let offset: usize =
                                        gep.rsplit("i64 ").next().unwrap().parse().unwrap();
                                    if offset >= 8 {
                                        initialized.insert(offset);
                                    }
                                }
                            }
                        }
                    }
                    if line == *header {
                        let prefix_slot = if base == 24 { 16 } else { 8 };
                        assert!(initialized.contains(&prefix_slot) && (0..slots).all(|slot| initialized.contains(&(base + slot * 8))), "seed before ALL slots/header metadata initialized: {raw}, slots={slots}, initialized={initialized:?}");
                        break;
                    }
                }
                if start <= header_at && header_at < end {
                    continue;
                }
                let preds = predecessors
                    .get(&block)
                    .expect("seed must follow allocation initialization");
                for pred in preds {
                    pending.push((
                        pred.clone(),
                        blocks[pred].1,
                        initialized.clone(),
                        visited.clone(),
                    ));
                }
            }
        }
    }
    assert!(
        arrays >= 3 && objects > 0,
        "both independent inline allocators must be live"
    );
    assert_eq!(
        sites,
        ir.lines()
            .filter(|l| l.contains("call ") && l.contains("@js_gc_note_black_birth("))
            .count(),
        "count EVERY inline slow site in the WHOLE module"
    );
    assert!(
        !ir.contains("call void @js_write_barrier_root_heap_word("),
        "no per-site root-shading workaround"
    );
}
