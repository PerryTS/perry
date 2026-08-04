//! Precise GC roots lowered onto the native frame (#7173 / #7174).
//!
//! Split out of `function.rs` only because of the 2,000-line cap; this is the
//! statepoint/RS4GC half of the module and nothing else moved with it. The
//! entry points are [`lower_precise_roots_to_native_stack`] and
//! [`retype_landing_pads_for_statepoints`], both called from
//! `LlFunction::serialize`.

fn parse_shadow_bind(line: &str) -> Option<(usize, String)> {
    let rest = line
        .trim()
        .strip_prefix("call void @js_shadow_slot_bind(i32 ")?;
    let (idx, ptr) = rest.split_once(", ptr ")?;
    let ptr = ptr.strip_suffix(')')?.trim();
    Some((idx.parse().ok()?, ptr.to_string()))
}

fn parse_shadow_set(line: &str) -> Option<(usize, String)> {
    let rest = line
        .trim()
        .strip_prefix("call void @js_shadow_slot_set(i32 ")?;
    let (idx, value) = rest.split_once(", i64 ")?;
    let value = value.strip_suffix(')')?.trim();
    Some((idx.parse().ok()?, value.to_string()))
}

/// Compute a conservative set of active logical shadow slots before each IR
/// line. Joins use union ("active on any incoming path"), so a stale local can
/// be retained but a live root cannot be omitted.
fn lower_roots_for_rs4gc(lines: &[&str], root_ptrs: &[String]) -> Option<String> {
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
                    crate::gc_call_effects::GcCallEffect::CannotCollect
                    | crate::gc_call_effects::GcCallEffect::NeverReturns => true,
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

fn direct_callee_name(line: &str) -> Option<&str> {
    let call = line.trim().split_once("call ")?.1;
    let args_open = call.find('(')?;
    let target = call[..args_open].trim();
    let name = target.split_ascii_whitespace().last()?.strip_prefix('@')?;
    (!name.is_empty()
        && name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.' | '$')))
    .then_some(name)
}

/// Lower this function's shadow-slot binding IR into RS4GC's input form.
///
/// There is one native-root backend. The explicit statepoint bridge — Perry
/// rewriting its own IR text into `gc.statepoint` calls with hand-emitted
/// relocations — is gone. It was strictly weaker than RS4GC (it could not root
/// an `invoke`, so it refused try-carrying functions outright) and it survived
/// only as this path's fallback. Measured before removing it: **1,574 functions
/// across `test-drizzle-pg` and the gc-ratchet probes all lowered as `rs4gc`,
/// none fell back.** A fallback nothing takes is an untested configuration,
/// which is exactly what the GC knob kill-policy exists to prevent.
///
/// A bail is therefore a hard failure, not a silent downgrade: if the
/// recognizer meets a root-alloca use it does not understand, anything else
/// would emit a frame whose roots the collector cannot find.
pub(super) fn lower_precise_roots_to_native_stack(
    ir: &str,
    function_name: &str,
    slot_count: u32,
) -> String {
    let lines: Vec<&str> = ir.lines().collect();
    let mut roots: Vec<Option<String>> = vec![None; slot_count as usize];
    for line in &lines {
        if let Some((idx, ptr)) = parse_shadow_bind(line) {
            if let Some(root) = roots.get_mut(idx) {
                match root {
                    Some(existing) => {
                        debug_assert_eq!(
                            existing, &ptr,
                            "one precise-root slot must not bind two native allocas"
                        );
                    }
                    None => *root = Some(ptr),
                }
            }
        }
    }

    let root_ptrs: Vec<String> = roots.iter().flatten().cloned().collect();
    let report = crate::statepoint_report::enabled().then(|| {
        crate::statepoint_report::FunctionRecord::new(
            function_name,
            "rs4gc",
            slot_count,
            root_ptrs.len(),
        )
    });

    // Runs BEFORE any empty-roots early return on purpose: a function can
    // reserve slots (so it carries `gc "statepoint-example"`) yet bind none,
    // and still contain inline asm that RS4GC would rewrite into an invalid
    // statepoint. Found on the Claude Code bundle, where an early return
    // skipped leaf-marking and the verifier aborted with "Cannot take the
    // address of an inline asm!".
    match lower_roots_for_rs4gc(&lines, &root_ptrs) {
        Some(out) => {
            if let Some(mut report) = report {
                report.note_call(root_ptrs.len());
                crate::statepoint_report::record(report);
            }
            out
        }
        None => panic!(
            "perry: native-root lowering could not recognise a root-alloca use in @{} \
             ({} root slots). This used to fall back to the explicit statepoint bridge, \
             which is gone; emitting anything else would produce a frame whose roots the \
             collector cannot find. Report the function shape on #7174.",
            function_name,
            root_ptrs.len(),
        ),
    }
}
pub(super) fn retype_landing_pads_for_statepoints(ir: &str) -> String {
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

/// Whether `line` mentions SSA register `reg` as a whole token rather than as
/// a prefix of a longer name (`%r2` must not match `%r21`).
fn mentions_register(line: &str, reg: &str) -> bool {
    let mut from = 0;
    while let Some(idx) = line[from..].find(reg) {
        let at = from + idx;
        let after = line[at + reg.len()..].chars().next();
        if !matches!(after, Some(c) if c.is_ascii_alphanumeric() || c == '_' || c == '.') {
            return true;
        }
        from = at + reg.len();
    }
    false
}

#[cfg(test)]
mod stack_map_tests {

    #[test]
    fn retypes_dead_landing_pads_for_rs4gc() {
        let ir = "define void @probe() {\n\
                  entry:\n\
                  %lp = landingpad { ptr, i32 } catch ptr null\n\
                  br label %next\n\
                  }\n";
        let out = super::retype_landing_pads_for_statepoints(ir);
        assert!(out.contains("%lp = landingpad token cleanup"), "{out}");
    }

    #[test]
    fn leaves_a_used_landing_pad_alone() {
        // If the pad's payload is read, retyping it to `token` would break the
        // consumer silently. Fail closed: RS4GC's loud verifier error is the
        // better outcome.
        let ir = "define void @probe() {\n\
                  entry:\n\
                  %lp = landingpad { ptr, i32 } catch ptr null\n\
                  %exn = extractvalue { ptr, i32 } %lp, 0\n\
                  br label %next\n\
                  }\n";
        let out = super::retype_landing_pads_for_statepoints(ir);
        assert!(
            out.contains("%lp = landingpad { ptr, i32 } catch ptr null"),
            "{out}"
        );
    }

    #[test]
    fn register_match_is_whole_token() {
        // `%r2` must not be considered used by a mention of `%r21`.
        assert!(super::mentions_register("  br label %r2", "%r2"));
        assert!(!super::mentions_register("  %x = add i64 %r21, 1", "%r2"));
    }
}
