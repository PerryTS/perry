//! Equivalence pin for the linear root lowering (#8583).
//!
//! The two `reference_*` functions below are the previous implementations,
//! copied verbatim (only renamed): `reference_lower_roots_for_rs4gc` iterated
//! every root pointer per line, `reference_retype_landing_pads_for_statepoints`
//! rescanned the whole function per landing pad. Both were O(lines × roots)
//! and together were the bulk of a 27-minute "partitioning" phase on the
//! 13.7 MB Claude Code bundle. The linear replacements must produce
//! byte-identical output; this module proves that on generated functions
//! that exercise every shape either implementation distinguishes, rather
//! than arguing it from the code.

use super::{direct_callee_name, mentions_register, parse_shadow_bind, parse_shadow_set};

/// Compute a conservative set of active logical shadow slots before each IR
/// line. Joins use union ("active on any incoming path"), so a stale local can
/// be retained but a live root cannot be omitted.
pub(super) fn reference_lower_roots_for_rs4gc(
    lines: &[&str],
    root_ptrs: &[String],
) -> Option<String> {
    let roots: std::collections::HashSet<&str> = root_ptrs.iter().map(String::as_str).collect();
    let mut out = String::with_capacity(lines.len() * 48 + root_ptrs.len() * 96);
    let mut cast_counter = 0usize;

    for line in lines {
        if parse_shadow_bind(line).is_some() || parse_shadow_set(line).is_some() {
            continue;
        }
        let trimmed = line.trim_start();

        // Root-alloca definition: retype + null-init (mem2reg needs a
        // dominating definition for paths that read before the first bind,
        // same reason the i64 zero-init existed).
        // Root locals are emitted as `alloca double` (the NaN-box home) or
        // occasionally `alloca i64`; both become an addrspace(1) slot.
        if let Some(reg) = trimmed
            .strip_suffix("= alloca i64")
            .or_else(|| trimmed.strip_suffix("= alloca double"))
            .map(str::trim_end)
            .filter(|reg| roots.contains(reg))
        {
            out.push_str(&format!("  {reg} = alloca ptr addrspace(1)\n"));
            out.push_str(&format!("  store ptr addrspace(1) null, ptr {reg}\n"));
            continue;
        }

        let mut handled = false;
        for ptr in root_ptrs {
            if let Some(rest) = trimmed.strip_prefix("store i64 ") {
                if let Some(value) = rest.strip_suffix(&format!(", ptr {ptr}")) {
                    let value = value.trim();
                    if value == "0" {
                        out.push_str(&format!("  store ptr addrspace(1) null, ptr {ptr}\n"));
                    } else {
                        cast_counter += 1;
                        out.push_str(&format!(
                            "  %rs4gc.s{cast_counter} = inttoptr i64 {value} to ptr addrspace(1)\n  store ptr addrspace(1) %rs4gc.s{cast_counter}, ptr {ptr}\n"
                        ));
                    }
                    handled = true;
                    break;
                }
            }
            if let Some(rest) = trimmed.strip_prefix("store double ") {
                if let Some(value) = rest.strip_suffix(&format!(", ptr {ptr}")) {
                    let value = value.trim();
                    cast_counter += 1;
                    out.push_str(&format!(
                        "  %rs4gc.b{cast_counter} = bitcast double {value} to i64\n  %rs4gc.s{cast_counter} = inttoptr i64 %rs4gc.b{cast_counter} to ptr addrspace(1)\n  store ptr addrspace(1) %rs4gc.s{cast_counter}, ptr {ptr}\n"
                    ));
                    handled = true;
                    break;
                }
            }
            if trimmed
                == format!(
                    "{} = load i64, ptr {ptr}",
                    trimmed.split(' ').next().unwrap_or("")
                )
            {
                let result = trimmed.split(' ').next().unwrap_or("");
                out.push_str(&format!(
                    "  {result}.rs4p = load ptr addrspace(1), ptr {ptr}\n  {result} = ptrtoint ptr addrspace(1) {result}.rs4p to i64\n"
                ));
                handled = true;
                break;
            }
            if trimmed
                == format!(
                    "{} = load double, ptr {ptr}",
                    trimmed.split(' ').next().unwrap_or("")
                )
            {
                let result = trimmed.split(' ').next().unwrap_or("");
                out.push_str(&format!(
                    "  {result}.rs4p = load ptr addrspace(1), ptr {ptr}\n  {result}.rs4i = ptrtoint ptr addrspace(1) {result}.rs4p to i64\n  {result} = bitcast i64 {result}.rs4i to double\n"
                ));
                handled = true;
                break;
            }
        }
        if handled {
            continue;
        }

        // Fail closed: any other appearance of a root alloca name.
        if root_ptrs.iter().any(|ptr| {
            line.contains(ptr.as_str())
                && line
                    .split(|c: char| !(c.is_alphanumeric() || c == '%' || c == '_' || c == '.'))
                    .any(|tok| tok == ptr)
        }) {
            return None;
        }

        // Audited non-collecting callees become RS4GC leaf calls: the pass
        // will not treat them as safepoints, transferring the call-effect
        // table wholesale. AllocNoReentry keeps its contract gating.
        let is_call = trimmed.starts_with("call ")
            || trimmed.contains(" = call ")
            || trimmed.starts_with("tail call ")
            || trimmed.contains(" = tail call ");
        // Inline asm must be marked leaf explicitly: RS4GC otherwise rewrites
        // it into a statepoint whose callee is the asm value, which the
        // verifier rejects outright ("Cannot take the address of an inline
        // asm!"). Found on the Claude Code bundle, where other codegen paths
        // emit zero-instruction asm barriers.
        if is_call && trimmed.ends_with(')') && trimmed.contains(" asm ") {
            out.push_str(line.trim_end());
            out.push_str(" \"gc-leaf-function\"\n");
            continue;
        }
        if is_call && trimmed.ends_with(')') && !trimmed.contains(" asm ") {
            if let Some(callee) = direct_callee_name(line) {
                let leaf = match crate::gc_call_effects::classify_direct_callee(callee) {
                    crate::gc_call_effects::GcCallEffect::CannotCollect => true,
                    crate::gc_call_effects::GcCallEffect::AllocNoReentry => {
                        crate::codegen::helpers::gc_safepoint_only_contract_enabled()
                    }
                    crate::gc_call_effects::GcCallEffect::Unknown => false,
                };
                if leaf && !callee.starts_with("llvm.") {
                    out.push_str(line.trim_end());
                    out.push_str(" \"gc-leaf-function\"\n");
                    continue;
                }
            }
        }

        out.push_str(line);
        out.push('\n');
    }
    Some(out)
}

pub(super) fn reference_retype_landing_pads_for_statepoints(ir: &str) -> String {
    const ITANIUM: &str = "landingpad { ptr, i32 } catch ptr null";
    if !ir.contains(ITANIUM) {
        return ir.to_string();
    }
    let mut out = String::with_capacity(ir.len());
    for line in ir.lines() {
        let rewritten = match line.split_once(" = ") {
            Some((reg, rest)) if rest.trim() == ITANIUM => {
                let reg = reg.trim();
                // Referenced anywhere else? Then its payload is live.
                let used = ir.lines().any(|other| {
                    !std::ptr::eq(other.as_ptr(), line.as_ptr()) && mentions_register(other, reg)
                });
                if used {
                    None
                } else {
                    Some(format!("{} = landingpad token cleanup", reg))
                }
            }
            _ => None,
        };
        match rewritten {
            Some(r) => out.push_str(&r),
            None => out.push_str(line),
        }
        out.push('\n');
    }
    out
}

/// Deterministic xorshift so a failure reproduces from its seed alone.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
    fn pick<'a>(&mut self, items: &[&'a str]) -> &'a str {
        items[self.below(items.len())]
    }
}

/// One generated function: its lines, and the root pointer list the lowering
/// would have derived from its `js_shadow_slot_bind` calls.
struct Generated {
    ir: String,
    root_ptrs: Vec<String>,
}

/// Names that share a prefix with roots (`%l_1` vs `%l_10`, `%lp2` vs
/// `%lp21`) are the traps the whole-token rules exist for, so every generator
/// draws from both roots and near-miss decoys.
/// `clean` keeps every root mention inside the recognized access shapes, so
/// the lowering runs to the end instead of bailing on the fail-closed scan —
/// the wide-function cases need that, or they time an early return.
fn generate(rng: &mut Rng, root_count: usize, line_count: usize, clean: bool) -> Generated {
    let roots: Vec<String> = (0..root_count).map(|i| format!("%l_{i}")).collect();
    // `%l_1a` shares `%l_1`'s prefix but is never itself a root (a numeric
    // suffix would collide with a higher-numbered root once there are >10).
    let decoys: Vec<String> = (0..root_count.max(1))
        .map(|i| format!("%l_{i}a"))
        .chain((0..3).map(|i| format!("%x_{i}")))
        .collect();
    let root_refs: Vec<&str> = roots.iter().map(String::as_str).collect();
    let decoy_refs: Vec<&str> = decoys.iter().map(String::as_str).collect();
    let any_ptr = |rng: &mut Rng| -> &str {
        if root_refs.is_empty() || rng.below(3) == 0 {
            rng.pick(&decoy_refs)
        } else {
            rng.pick(&root_refs)
        }
    };
    let values = ["0", "%v1", "%v2", "ptrtoint (ptr @g to i64)"];
    let callees = [
        "@may_collect()",
        "@js_map_alloc(i32 0)",
        "@js_box_set_bits(i64 %a, i64 1)",
        "@js_closure_get_capture_bits(i64 0, i32 0)",
        "@llvm.lifetime.start(i64 8, ptr %v1)",
    ];

    let mut ir = String::from("define void @f() {\nentry:\n");
    let mut root_ptrs = Vec::new();
    // Every root is bound at least once so it is in `root_ptrs`; bind order
    // (and therefore `root_ptrs` order) is randomized because the reference
    // iterated roots in that order.
    let mut bind_order: Vec<usize> = (0..root_count).collect();
    for i in (1..bind_order.len()).rev() {
        let j = rng.below(i + 1);
        bind_order.swap(i, j);
    }
    for &i in &bind_order {
        ir.push_str(&format!(
            "  {} = alloca {}\n",
            roots[i],
            if rng.below(2) == 0 { "i64" } else { "double" }
        ));
        ir.push_str(&format!(
            "  call void @js_shadow_slot_bind(i32 {i}, ptr {})\n",
            roots[i]
        ));
        root_ptrs.push(roots[i].clone());
    }
    for d in &decoys {
        ir.push_str(&format!("  {d} = alloca i64\n"));
    }
    let mut reg = 0usize;
    let mut pads: Vec<String> = Vec::new();
    for _ in 0..line_count {
        reg += 1;
        // An `align` suffix on a ROOT access is a fail-closed shape in both
        // implementations (the pointer operand must be the whole tail), so a
        // clean function only puts it on decoy accesses.
        let ptr = any_ptr(rng);
        let align = if rng.below(4) == 0 && !(clean && root_refs.contains(&ptr)) {
            ", align 8"
        } else {
            ""
        };
        let line = match rng.below(16) {
            0 => format!("  store i64 {}, ptr {ptr}{align}", rng.pick(&values)),
            1 => format!("  store double %d{}, ptr {ptr}{align}", reg),
            2 => format!("  %r{reg} = load i64, ptr {ptr}{align}"),
            3 => format!("  %r{reg} = load double, ptr {ptr}{align}"),
            4 => format!(
                "  call void @js_shadow_slot_set(i32 {}, i64 %v1)",
                rng.below(root_count.max(1))
            ),
            5 => format!("  call void {}", rng.pick(&callees)),
            6 => format!("  %r{reg} = call i64 {}", rng.pick(&callees)),
            7 => format!("  %r{reg} = tail call i64 {}", rng.pick(&callees)),
            8 => "  call void asm sideeffect \"\", \"\"()".to_string(),
            9 if clean => format!("  %r{reg} = ptrtoint ptr {} to i64", rng.pick(&decoy_refs)),
            9 => format!("  %r{reg} = ptrtoint ptr {} to i64", any_ptr(rng)),
            10 => format!("  %r{reg} = add i64 %r{}, 1", reg.saturating_sub(1)),
            11 => {
                let pad = format!("%lp{reg}");
                pads.push(pad.clone());
                format!("  {pad} = landingpad {{ ptr, i32 }} catch ptr null")
            }
            12 if !pads.is_empty() => {
                let pad = rng.pick(&pads.iter().map(String::as_str).collect::<Vec<_>>());
                if rng.below(2) == 0 {
                    format!("  %e{reg} = extractvalue {{ ptr, i32 }} {pad}, 0")
                } else {
                    // prefix trap: mentions `%lpN1`, never `%lpN`
                    format!("  br label {pad}1")
                }
            }
            13 => format!("  br label %next{}", reg),
            14 => format!(
                "  store i64 {}, ptr {}, ptr {}",
                rng.pick(&values),
                rng.pick(&decoy_refs),
                any_ptr(rng)
            ),
            _ if clean => format!("  %r{reg} = load i64 , ptr {}", rng.pick(&decoy_refs)),
            _ => format!("  %r{reg} = load i64 , ptr {}", any_ptr(rng)),
        };
        ir.push_str(&line);
        ir.push('\n');
    }
    ir.push_str("  ret void\n}\n");
    Generated { ir, root_ptrs }
}

#[test]
fn linear_lowering_matches_the_per_root_reference_on_generated_functions() {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let mut lowered_some = 0usize;
    for case in 0..600usize {
        let roots = rng.below(6);
        let lines = 1 + rng.below(40);
        let seed = rng.0;
        let g = generate(&mut rng, roots, lines, case % 2 == 0);
        let lines_vec: Vec<&str> = g.ir.lines().collect();
        let reference = reference_lower_roots_for_rs4gc(&lines_vec, &g.root_ptrs);
        let linear = super::lower_roots_for_rs4gc(&lines_vec, &g.root_ptrs);
        assert_eq!(
            linear, reference,
            "case {case} (seed {seed:#x}) diverged from the reference lowering:\n{}",
            g.ir
        );
        lowered_some += usize::from(linear.is_some());
    }
    // The generator must exercise both verdicts, or the equality above could
    // be vacuous (every case bailing on the fail-closed scan).
    assert!(
        lowered_some > 100,
        "only {lowered_some} of 600 cases lowered"
    );
}

#[test]
fn linear_landing_pad_retype_matches_the_rescan_reference() {
    let mut rng = Rng(0xD1B5_4A32_D192_ED03);
    let mut retyped = 0usize;
    let mut kept = 0usize;
    for case in 0..400usize {
        let seed = rng.0;
        let roots = rng.below(3);
        let lines = 1 + rng.below(40);
        let g = generate(&mut rng, roots, lines, false);
        let reference = reference_retype_landing_pads_for_statepoints(&g.ir);
        let linear = super::retype_landing_pads_for_statepoints(&g.ir);
        assert_eq!(
            linear, reference,
            "case {case} (seed {seed:#x}) diverged from the reference retype:\n{}",
            g.ir
        );
        retyped += linear.matches("landingpad token cleanup").count();
        kept += linear
            .matches("landingpad { ptr, i32 } catch ptr null")
            .count();
    }
    assert!(
        retyped > 0 && kept > 0,
        "generator must produce both dead ({retyped}) and live ({kept}) pads"
    );
}

/// The reason for the rewrite, pinned as a budget rather than a clock: the
/// reference form does `roots × lines` suffix constructions; the linear one
/// does `lines` parses. A regression to per-root iteration would make this
/// case take minutes, which is loud enough without a timing assertion.
#[test]
fn linear_lowering_handles_a_wide_function_without_per_root_work() {
    let mut rng = Rng(0xABCD_EF01_2345_6789);
    let g = generate(&mut rng, 2_000, 40_000, true);
    let lines_vec: Vec<&str> = g.ir.lines().collect();
    let started = std::time::Instant::now();
    let linear = super::lower_roots_for_rs4gc(&lines_vec, &g.root_ptrs);
    let retyped = super::retype_landing_pads_for_statepoints(&g.ir);
    let elapsed = started.elapsed();
    assert!(
        linear.is_some(),
        "a clean wide function must lower end to end"
    );
    assert!(!retyped.is_empty());
    eprintln!("wide function (2000 roots × ~44k lines) lowered in {elapsed:?}");
}

/// For the record, not for CI: the same wide function through the reference
/// (per-root) form. Run with `--ignored --nocapture` to reproduce the number
/// quoted in the PR that introduced the linear lowering.
#[test]
#[ignore]
fn reference_timing_on_the_wide_function() {
    let mut rng = Rng(0xABCD_EF01_2345_6789);
    let g = generate(&mut rng, 2_000, 40_000, true);
    let lines_vec: Vec<&str> = g.ir.lines().collect();
    let started = std::time::Instant::now();
    let reference = reference_lower_roots_for_rs4gc(&lines_vec, &g.root_ptrs);
    let lowering = started.elapsed();
    let started = std::time::Instant::now();
    let retyped = reference_retype_landing_pads_for_statepoints(&g.ir);
    let retype = started.elapsed();
    assert_eq!(
        reference,
        super::lower_roots_for_rs4gc(&lines_vec, &g.root_ptrs)
    );
    assert_eq!(retyped, super::retype_landing_pads_for_statepoints(&g.ir));
    eprintln!("reference lowering {lowering:?}, reference retype {retype:?}");
}
