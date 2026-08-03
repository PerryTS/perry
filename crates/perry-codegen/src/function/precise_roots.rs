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
fn stack_map_active_slots(
    lines: &[&str],
    slot_count: u32,
) -> Vec<Option<std::collections::HashSet<usize>>> {
    use std::collections::{HashMap, HashSet, VecDeque};

    #[derive(Debug)]
    struct Block {
        first_line: usize,
        end_line: usize,
        successors: Vec<usize>,
    }

    fn label_name(line: &str) -> Option<&str> {
        if line.starts_with(char::is_whitespace) {
            return None;
        }
        line.strip_suffix(':')
            .filter(|name| !name.is_empty() && !name.starts_with(';'))
    }

    fn referenced_labels(line: &str) -> Vec<&str> {
        let mut labels = Vec::new();
        let mut rest = line;
        while let Some(pos) = rest.find("label %") {
            let after = &rest[pos + "label %".len()..];
            let len = after
                .bytes()
                .take_while(|byte| {
                    byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b'$')
                })
                .count();
            if len == 0 {
                break;
            }
            labels.push(&after[..len]);
            rest = &after[len..];
        }
        labels
    }

    let labels: Vec<(usize, &str)> = lines
        .iter()
        .enumerate()
        .filter_map(|(idx, line)| label_name(line).map(|name| (idx, name)))
        .collect();
    let mut states = vec![None; lines.len()];
    if labels.is_empty() {
        return states;
    }

    let label_to_block: HashMap<&str, usize> = labels
        .iter()
        .enumerate()
        .map(|(block, (_, name))| (*name, block))
        .collect();
    let mut blocks: Vec<Block> = labels
        .iter()
        .enumerate()
        .map(|(block, (label_line, _))| Block {
            first_line: label_line + 1,
            end_line: labels
                .get(block + 1)
                .map_or(lines.len(), |(next_line, _)| *next_line),
            successors: Vec::new(),
        })
        .collect();
    for block in &mut blocks {
        let mut seen = HashSet::new();
        for line in &lines[block.first_line..block.end_line] {
            for label in referenced_labels(line) {
                if let Some(&successor) = label_to_block.get(label) {
                    if seen.insert(successor) {
                        block.successors.push(successor);
                    }
                }
            }
        }
    }

    fn apply_root_op(state: &mut HashSet<usize>, line: &str, slot_count: u32) {
        if let Some((idx, _)) = parse_shadow_bind(line) {
            if idx < slot_count as usize {
                state.insert(idx);
            }
        } else if let Some((idx, value)) = parse_shadow_set(line) {
            if idx < slot_count as usize {
                if value == "0" {
                    state.remove(&idx);
                } else {
                    state.insert(idx);
                }
            }
        }
    }

    let mut entries: Vec<Option<HashSet<usize>>> = vec![None; blocks.len()];
    entries[0] = Some(HashSet::new());
    let mut work = VecDeque::from([0usize]);
    while let Some(block_idx) = work.pop_front() {
        let Some(mut state) = entries[block_idx].clone() else {
            continue;
        };
        let block = &blocks[block_idx];
        for line in &lines[block.first_line..block.end_line] {
            apply_root_op(&mut state, line, slot_count);
        }
        for &successor in &block.successors {
            let changed = match &mut entries[successor] {
                Some(existing) => {
                    let old_len = existing.len();
                    existing.extend(state.iter().copied());
                    existing.len() != old_len
                }
                entry @ None => {
                    *entry = Some(state.clone());
                    true
                }
            };
            if changed {
                work.push_back(successor);
            }
        }
    }

    for (block_idx, block) in blocks.iter().enumerate() {
        let Some(mut state) = entries[block_idx].clone() else {
            continue;
        };
        for (line_idx, line) in lines
            .iter()
            .enumerate()
            .take(block.end_line)
            .skip(block.first_line)
        {
            states[line_idx] = Some(state.clone());
            apply_root_op(&mut state, line, slot_count);
        }
    }
    states
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum PreciseRootBackend {
    Statepoint,
    /// `PERRY_RS4GC=1` (#7174): retype every root alloca to
    /// `ptr addrspace(1)` with cast surgery at its load/store sites, tag the
    /// function `gc "statepoint-example"`, mark audited non-collecting
    /// callees `"gc-leaf-function"` at the call site, and emit NO per-call
    /// safepoint machinery — `opt -passes='function(mem2reg),
    /// rewrite-statepoints-for-gc'` promotes the allocas to SSA and inserts
    /// every statepoint, relocation, and downstream-use rewrite itself.
    /// After mem2reg, each former load site is a cast site, which is exactly
    /// the placement RS4GC needs to rewrite uses with relocated values.
    /// Fail-closed: any use of a root alloca outside the recognized
    /// load/store shapes bails the whole function to the Statepoint backend.
    Rs4gc,
}

impl PreciseRootBackend {
    fn as_str(self) -> &'static str {
        match self {
            Self::Statepoint => "statepoint",
            Self::Rs4gc => "rs4gc",
        }
    }
}

/// RS4GC surgery (#7174): retype root allocas to `ptr addrspace(1)` and cast
/// at every recognized load/store site. Returns `None` when any root alloca
/// appears in an unrecognized shape (the caller falls back to the explicit
/// statepoint backend for the whole function).
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

#[derive(Debug, Eq, PartialEq)]
struct DirectCall<'a> {
    result: Option<&'a str>,
    return_type: &'a str,
    callee: &'a str,
    args: Vec<&'a str>,
    arg_types: Vec<&'a str>,
}

fn split_call_args(args: &str) -> Option<Vec<&str>> {
    if args.trim().is_empty() {
        return Some(Vec::new());
    }
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut start = 0usize;
    for (idx, ch) in args.char_indices() {
        match ch {
            '(' | '[' | '{' | '<' => depth += 1,
            ')' | ']' | '}' | '>' => {
                depth -= 1;
                if depth < 0 {
                    return None;
                }
            }
            ',' if depth == 0 => {
                out.push(args[start..idx].trim());
                start = idx + 1;
            }
            _ => {}
        }
    }
    if depth != 0 {
        return None;
    }
    out.push(args[start..].trim());
    Some(out)
}

fn statepoint_scalar_type(arg: &str) -> Option<&str> {
    let ty = arg.split_ascii_whitespace().next()?;
    matches!(
        ty,
        "i1" | "i8" | "i16" | "i32" | "i64" | "i128" | "float" | "double" | "ptr"
    )
    .then_some(ty)
}

/// Parse the deliberately small direct-call subset emitted by `LlBlock`.
///
/// Calls with tail markers, operand attributes, aggregate types, inline asm,
/// indirect targets, or call-site suffixes stay on the plain stack-map
/// fallback. That keeps the research mode correct while making its explicit
/// statepoint coverage measurable and easy to expand.
fn parse_direct_statepoint_call(line: &str) -> Option<DirectCall<'_>> {
    let trimmed = line.trim();
    let (result, call) = if let Some(call) = trimmed.strip_prefix("call ") {
        (None, call)
    } else {
        let (result, call) = trimmed.split_once(" = call ")?;
        (Some(result.trim()), call)
    };
    let (return_type, target_and_args) = call.split_once(' ')?;
    if !matches!(
        return_type,
        "void" | "i1" | "i8" | "i16" | "i32" | "i64" | "i128" | "float" | "double" | "ptr"
    ) {
        return None;
    }
    if return_type != "void" && result.is_none() {
        return None;
    }
    let open = target_and_args.find('(')?;
    let close = target_and_args.rfind(')')?;
    if close + 1 != target_and_args.len() {
        return None;
    }
    let callee = target_and_args[..open].trim();
    // Indirect targets are statepoint-able: `gc.statepoint` takes the callee as
    // a `ptr` operand, and `emit_statepoint` interpolates it verbatim, so
    // `ptr elementtype(T) %fnptr` is as valid as `... @callee`. Rejecting them
    // was a limitation of this textual parser, not of statepoints — and the
    // fallback it forced is the unsound plain stack map. An unknown callee
    // simply cannot be audited as non-collecting, which is the conservative
    // (correct) answer anyway.
    let direct = callee.starts_with('@');
    let indirect = callee.starts_with('%');
    if !(direct || indirect)
        || callee.starts_with("@llvm.")
        || matches!(callee, "@setjmp" | "@_setjmp" | "@longjmp" | "@_longjmp")
    {
        return None;
    }
    let args = split_call_args(&target_and_args[open + 1..close])?;
    let arg_types = args
        .iter()
        .map(|arg| statepoint_scalar_type(arg))
        .collect::<Option<Vec<_>>>()?;
    Some(DirectCall {
        result,
        return_type,
        callee,
        args,
        arg_types,
    })
}

/// Return a direct callee name without the leading `@`.
///
/// This accepts more call syntax than the statepoint parser because the
/// GC-effect audit only needs to recognize a direct target. Unsupported and
/// indirect forms return `None` and therefore stay conservative.
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

fn gc_result_suffix(ty: &str) -> Option<&'static str> {
    match ty {
        "i1" => Some("i1"),
        "i8" => Some("i8"),
        "i16" => Some("i16"),
        "i32" => Some("i32"),
        "i64" => Some("i64"),
        "i128" => Some("i128"),
        "float" => Some("f32"),
        "double" => Some("f64"),
        "ptr" => Some("p0"),
        _ => None,
    }
}

/// Emit one explicit statepoint relocation sequence.
///
/// Perry roots remain ordinary NaN-boxed `i64` values everywhere else. At
/// this boundary we load each live word, carry its exact bits through a
/// temporary addrspace(1) pointer, and convert the `gc.relocate` result back
/// into the existing slot. LLVM therefore owns the spill/reload and the
/// post-safepoint SSA transition without requiring a whole-program
/// representation change for this prototype.
fn emit_statepoint(out: &mut String, call: &DirectCall<'_>, live: &[&String], statepoint_id: u64) {
    for (root_idx, ptr) in live.iter().enumerate() {
        out.push_str(&format!(
            "  %perry_sp_bits_{statepoint_id}_{root_idx} = load i64, ptr {ptr}\n"
        ));
        out.push_str(&format!(
            "  %perry_sp_root_{statepoint_id}_{root_idx} = inttoptr i64 \
             %perry_sp_bits_{statepoint_id}_{root_idx} to ptr addrspace(1)\n"
        ));
    }

    let function_type = format!("{} ({})", call.return_type, call.arg_types.join(", "));
    let call_args = call
        .args
        .iter()
        .map(|arg| format!(", {arg}"))
        .collect::<String>();
    let gc_live = live
        .iter()
        .enumerate()
        .map(|(root_idx, _)| format!("ptr addrspace(1) %perry_sp_root_{statepoint_id}_{root_idx}"))
        .collect::<Vec<_>>()
        .join(", ");
    out.push_str(&format!(
        "  %perry_sp_token_{statepoint_id} = call token (i64, i32, ptr, i32, i32, ...) \
         @llvm.experimental.gc.statepoint.p0(i64 {statepoint_id}, i32 0, \
         ptr elementtype({function_type}) {}, i32 {}, i32 0{call_args}, i32 0, i32 0) \
         [\"gc-live\"({gc_live})]\n",
        call.callee,
        call.args.len()
    ));

    if let Some(result) = call.result {
        let suffix = gc_result_suffix(call.return_type)
            .expect("non-void statepoint return type was validated by the parser");
        out.push_str(&format!(
            "  {result} = call {} @llvm.experimental.gc.result.{suffix}(token \
             %perry_sp_token_{statepoint_id})\n",
            call.return_type
        ));
    }

    for (root_idx, ptr) in live.iter().enumerate() {
        out.push_str(&format!(
            "  %perry_sp_relocated_{statepoint_id}_{root_idx} = call ptr addrspace(1) \
             @llvm.experimental.gc.relocate.p1(token %perry_sp_token_{statepoint_id}, \
             i32 {root_idx}, i32 {root_idx})\n"
        ));
        out.push_str(&format!(
            "  %perry_sp_relocated_bits_{statepoint_id}_{root_idx} = ptrtoint \
             ptr addrspace(1) %perry_sp_relocated_{statepoint_id}_{root_idx} to i64\n"
        ));
        out.push_str(&format!(
            "  store i64 %perry_sp_relocated_bits_{statepoint_id}_{root_idx}, ptr {ptr}\n"
        ));
    }
}

/// Lower Perry's existing precise-root operations to native-stack metadata.
///
/// The old binding calls already name exactly the mutable native alloca that a
/// moving collection must rewrite. We use them as compile-time markers:
///
/// * collect `logical slot -> native alloca`;
/// * remove the runtime bind calls and shadow-frame traffic;
/// * compute conservative per-call liveness from bind/clear markers without
///   mutating the native slot;
/// * either place a plain stack map before a call, or replace a supported call
///   with a statepoint/result/relocate sequence.
///
/// Statepoint mode deliberately retains a plain-stack-map fallback for call
/// forms outside the narrow parser above. The fallback preserves correctness
/// while the report records how much of real Perry code reaches the explicit
/// relocation path.
pub(super) fn lower_precise_roots_to_native_stack(
    ir: &str,
    function_name: &str,
    slot_count: u32,
    backend: PreciseRootBackend,
) -> String {
    let lines: Vec<&str> = ir.lines().collect();
    let active_slots = stack_map_active_slots(&lines, slot_count);
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

    let slot_roots = roots;
    let root_ptrs: Vec<String> = slot_roots.iter().flatten().cloned().collect();
    let mut report = crate::statepoint_report::enabled().then(|| {
        crate::statepoint_report::FunctionRecord::new(
            function_name,
            backend.as_str(),
            slot_count,
            root_ptrs.len(),
        )
    });
    // RS4GC runs BEFORE the empty-roots early return on purpose: a function
    // can reserve slots (so it carries `gc "statepoint-example"`) yet bind
    // none, and it still contains inline asm that RS4GC would rewrite into an
    // invalid statepoint. Found on the Claude Code bundle, where the early
    // return skipped leaf-marking and the verifier aborted with "Cannot take
    // the address of an inline asm!".
    if backend == PreciseRootBackend::Rs4gc {
        if let Some(out) = lower_roots_for_rs4gc(&lines, &root_ptrs) {
            if let Some(mut report) = report {
                report.note_call(root_ptrs.len());
                crate::statepoint_report::record(report);
            }
            return out;
        }
        return lower_precise_roots_to_native_stack(
            ir,
            function_name,
            slot_count,
            PreciseRootBackend::Statepoint,
        );
    }

    if root_ptrs.is_empty() {
        let out = ir
            .lines()
            .filter(|line| parse_shadow_bind(line).is_none() && parse_shadow_set(line).is_none())
            .map(|line| format!("{line}\n"))
            .collect();
        if let Some(report) = report {
            crate::statepoint_report::record(report);
        }
        return out;
    }

    let mut out = String::with_capacity(ir.len() + root_ptrs.len() * 128);
    let mut available = std::collections::HashSet::<String>::new();
    let mut initialized = std::collections::HashSet::<String>::new();
    let mut map_id = 0u64;

    for (line_idx, line) in lines.iter().enumerate() {
        if parse_shadow_bind(line).is_some() {
            // Compile-time marker only. The real slot is already populated by
            // the local store immediately preceding this old bind.
            continue;
        }
        if parse_shadow_set(line).is_some() {
            // This marker changes stack-map liveness, not the program local.
            // Shadow-stack clears only flipped SLOT_ACTIVE for the same
            // reason: a value can be semantically read after its final
            // GC-capable call.
            continue;
        }

        // A stack-map operand must dominate the intrinsic. Root allocas are
        // normally entry-hoisted, but tracking definitions here also handles
        // the few block-local scalar-replacement slots without emitting
        // invalid SSA.
        for ptr in &root_ptrs {
            if line.trim_start().starts_with(&format!("{ptr} = ")) {
                available.insert(ptr.clone());
            }
        }

        out.push_str(line);
        out.push('\n');

        // Slots can be named by a stack map before their source-level `let`
        // executes. Zero them directly after the alloca so an earlier
        // safepoint never exposes uninitialized stack bytes as roots.
        for ptr in &root_ptrs {
            if available.contains(ptr)
                && !initialized.contains(ptr)
                && line.trim_start().starts_with(&format!("{ptr} = alloca "))
            {
                out.push_str(&format!("  store i64 0, ptr {ptr}\n"));
                initialized.insert(ptr.clone());
            }
        }

        // Insert before calls, not after. Rebuild the tail when the line just
        // appended is a call so the intrinsic's instruction offset is the
        // actual call-site offset in the final machine function.
        let trimmed = line.trim_start();
        let is_call = trimmed.starts_with("call ")
            || trimmed.contains(" = call ")
            || trimmed.starts_with("tail call ")
            || trimmed.contains(" = tail call ");
        if !is_call || trimmed.contains("@llvm.experimental.stackmap") {
            continue;
        }
        let active = active_slots.get(line_idx).and_then(Option::as_ref);
        let live: Vec<&String> = slot_roots
            .iter()
            .enumerate()
            .filter(|(idx, _)| active.is_some_and(|slots| slots.contains(idx)))
            .filter_map(|(_, ptr)| ptr.as_ref())
            .filter(|ptr| available.contains(*ptr) && initialized.contains(*ptr))
            .collect();
        if let Some(report) = report.as_mut() {
            report.note_call(live.len());
        }
        if live.is_empty() {
            continue;
        }

        let direct_callee = direct_callee_name(line);
        let is_compiler_only = direct_callee.is_some_and(|callee| callee.starts_with("llvm."))
            || trimmed.contains("call void asm ");
        let cannot_collect = direct_callee.is_some_and(|callee| {
            match crate::gc_call_effects::classify_direct_callee(callee) {
                crate::gc_call_effects::GcCallEffect::CannotCollect => true,
                // Control never returns here: no relocation is consumed and
                // the frame's roots are dead past the call. Deeper frames
                // carry their own records.
                crate::gc_call_effects::GcCallEffect::NeverReturns => true,
                // Under the explicit-safepoint contract the runtime
                // guarantees these helpers' triggers never consume this
                // frame's precise roots (they defer to a declared safepoint
                // or collect behind a forced conservative scan), so the
                // call site needs no metadata. Without the contract they
                // stay safepoints.
                crate::gc_call_effects::GcCallEffect::AllocNoReentry => {
                    crate::codegen::helpers::gc_safepoint_only_contract_enabled()
                }
                crate::gc_call_effects::GcCallEffect::Unknown => false,
            }
        });
        if is_compiler_only || cannot_collect {
            // LLVM intrinsics, zero-instruction compiler barriers, and
            // runtime helpers in the audited GC-effect table cannot enter
            // Perry's allocator. Neither native-stack backend needs metadata
            // around them.
            if let Some(report) = report.as_mut() {
                report.note_skipped(direct_callee.unwrap_or("<inline-asm>"));
            }
            continue;
        }

        // Move the call line behind the intrinsic.
        let call_len = line.len() + 1;
        out.truncate(out.len() - call_len);
        if backend == PreciseRootBackend::Statepoint {
            if let Some(call) = parse_direct_statepoint_call(line) {
                emit_statepoint(&mut out, &call, &live, map_id);
                if let Some(report) = report.as_mut() {
                    report.note_statepoint(call.callee.trim_start_matches('@'), live.len());
                }
                map_id += 1;
                continue;
            }
        }
        // No statepoint could be formed for a call that has live roots. The
        // old behaviour was to fall back to a plain `llvm.experimental.stackmap`,
        // which is UNSOUND: LLVM may record a root slot's address as
        // `Register R#N`, caller-saved and unrecoverable at collection time,
        // so the collector silently misses that root.
        //
        // Measured on test-drizzle-pg (133 modules): 23,301 safepoints, ALL
        // statepoints, 0 plain stack maps, 0 parser fallbacks. The path is not
        // taken by real code, so failing closed costs nothing and removes the
        // last way this backend can lose a root. A loud compile failure beats
        // silent heap corruption.
        panic!(
            "perry: native-root lowering could not express a safepoint for \
             `{}` in @{} ({} live roots). Falling back to a plain stack map \
             here would record roots in caller-saved registers that the \
             collector cannot recover, so the compile stops instead. Report \
             this call shape on #7174.",
            direct_callee.unwrap_or("<indirect-or-unsupported>"),
            function_name,
            live.len(),
        );
    }
    if let Some(report) = report {
        crate::statepoint_report::record(report);
    }
    out
}

/// Retype Itanium landing pads to `token` for `statepoint-example`.
///
/// RS4GC uses the unwind destination's landing pad **as the token** for the
/// relocates it inserts on the exceptional edge, so the pad must already be
/// `landingpad token`. Given `{ ptr, i32 }` it emits
/// `gc.relocate({ ptr, i32 } %lpad, ...)` and the verifier rejects the module,
/// which is why a try-carrying function failed to compile under RS4GC at all.
///
/// This is only sound because the pad's value is **dead**: `try_stmt` emits it
/// to anchor the edge and branches straight on, taking the exception from the
/// runtime rather than the pad payload. So a pad whose register IS referenced
/// is left alone — retyping a value someone reads would swap a silent
/// miscompile for the loud one this fixes.
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
    use super::{
        direct_callee_name, lower_precise_roots_to_native_stack, parse_direct_statepoint_call,
        PreciseRootBackend,
    };

    fn lower_statepoints(input: &str, slots: u32) -> String {
        lower_precise_roots_to_native_stack(input, "probe", slots, PreciseRootBackend::Statepoint)
    }

    #[test]
    fn lowers_bind_and_liveness_clear_to_native_frame_maps() {
        let input = r#"define i64 @probe(i64 %arg) {
entry.0:
  %r0 = alloca i64
  store i64 %arg, ptr %r0
  call void @js_shadow_slot_bind(i32 0, ptr %r0)
  %r1 = call i64 @may_collect()
  call void @js_shadow_slot_set(i32 0, i64 0)
  call void @may_collect_again()
  ret i64 %r1
}
"#;
        let output = lower_statepoints(input, 1);
        assert!(!output.contains("@js_shadow_slot_bind"));
        assert!(!output.contains("@js_shadow_slot_set"));
        assert!(output.contains("%r0 = alloca i64\n  store i64 0, ptr %r0"));
        assert!(
            output.contains("@llvm.experimental.gc.statepoint.p0"),
            "the collecting call must become a statepoint:\n{output}"
        );
        assert!(
            output.contains("%r0"),
            "the root slot must appear in the statepoint's live list:\n{output}"
        );
        assert_eq!(output.matches("store i64 0, ptr %r0").count(), 1);
        assert_eq!(
            output
                .matches("@llvm.experimental.gc.statepoint.p0")
                .count(),
            1
        );
        assert!(output.contains("call void @may_collect_again()"));
    }

    #[test]
    fn does_not_reference_a_root_before_its_alloca_dominates() {
        let input = r#"define void @probe() {
entry.0:
  call void @early_call()
  %r0 = alloca i64
  call void @js_shadow_slot_bind(i32 0, ptr %r0)
  call void @late_call()
  ret void
}
"#;
        let output = lower_statepoints(input, 1);
        let early = output.find("call void @early_call()").unwrap();
        let first_map = output.find("@llvm.experimental.gc.statepoint.p0").unwrap();
        assert!(
            early < first_map,
            "no safepoint may reference a root before its alloca dominates:\n{output}"
        );
        assert!(
            output.contains("@late_call"),
            "the dominated call must still be mapped:\n{output}"
        );
    }

    #[test]
    fn unions_root_liveness_at_control_flow_joins() {
        let input = r#"define void @probe(i1 %cond) {
entry.0:
  %r0 = alloca i64
  call void @js_shadow_slot_bind(i32 0, ptr %r0)
  br i1 %cond, label %live.1, label %dead.2
live.1:
  call void @live_call()
  br label %merge.3
dead.2:
  call void @js_shadow_slot_set(i32 0, i64 0)
  call void @dead_call()
  br label %merge.3
merge.3:
  call void @merge_call()
  ret void
}
"#;
        let output = lower_statepoints(input, 1);
        assert!(output.contains("@llvm.experimental.gc.statepoint.p0"));
        assert!(!output.contains("@dead_call, ptr %r0"));
        assert!(output.contains("@merge_call"));
    }

    #[test]
    fn parses_the_scalar_direct_call_subset() {
        assert_eq!(
            direct_callee_name("  %r7 = call double @foo(i64 %r1, ptr %r2)"),
            Some("foo")
        );
        assert_eq!(
            direct_callee_name("  %r7 = call i64 ()* %fn()"),
            None,
            "an indirect target must not be inferred from its arguments"
        );
        assert_eq!(
            parse_direct_statepoint_call("  %r7 = call double @foo(i64 %r1, ptr %r2)"),
            Some(super::DirectCall {
                result: Some("%r7"),
                return_type: "double",
                callee: "@foo",
                args: vec!["i64 %r1", "ptr %r2"],
                arg_types: vec!["i64", "ptr"],
            })
        );
        assert!(parse_direct_statepoint_call(
            "  %r7 = call double (i64, ptr)* %fn(i64 %r1, ptr %r2)"
        )
        .is_none());
        assert!(parse_direct_statepoint_call("  call void @llvm.assume(i1 %ok)").is_none());
        assert!(parse_direct_statepoint_call("  %r7 = tail call i64 @foo()").is_none());
    }

    #[test]
    fn lowers_direct_calls_to_explicit_statepoint_relocations() {
        let input = r#"define i64 @probe(i64 %arg) {
entry.0:
  %r0 = alloca i64
  store i64 %arg, ptr %r0
  call void @js_shadow_slot_bind(i32 0, ptr %r0)
  %r1 = call i64 @may_collect(i64 %arg)
  ret i64 %r1
}
"#;
        let output = lower_statepoints(input, 1);
        assert!(!output.contains("call i64 @may_collect"));
        assert!(!output.contains("asm sideeffect"));
        assert!(output
            .contains("%perry_sp_root_0_0 = inttoptr i64 %perry_sp_bits_0_0 to ptr addrspace(1)"));
        assert!(output.contains(
            "ptr elementtype(i64 (i64)) @may_collect, i32 1, i32 0, i64 %arg, i32 0, i32 0"
        ));
        assert!(output
            .contains("%r1 = call i64 @llvm.experimental.gc.result.i64(token %perry_sp_token_0)"));
        assert!(output
            .contains("@llvm.experimental.gc.relocate.p1(token %perry_sp_token_0, i32 0, i32 0)"));
        assert!(output.contains("store i64 %perry_sp_relocated_bits_0_0, ptr %r0"));
    }

    #[test]
    fn statepoint_mode_maps_indirect_calls() {
        // An indirect call used to fall back to a plain stack map, which is the
        // unsound lowering: LLVM may record the root's address in a
        // caller-saved register. `gc.statepoint` takes its callee as a `ptr`
        // operand, so an indirect target is expressible — the restriction was
        // in this textual parser, not in statepoints.
        let input = r#"define i64 @probe(i64 %arg, ptr %fn) {
entry.0:
  %r0 = alloca i64
  store i64 %arg, ptr %r0
  call void @js_shadow_slot_bind(i32 0, ptr %r0)
  %r1 = call i64 %fn()
  ret i64 %r1
}
"#;
        let output = lower_statepoints(input, 1);
        assert!(
            output.contains("@llvm.experimental.gc.statepoint.p0"),
            "an indirect call with live roots must become a statepoint:\n{output}"
        );
        assert!(
            output.contains("%fn"),
            "the indirect target must survive as the statepoint callee:\n{output}"
        );
        assert!(
            !output.contains("@llvm.experimental.stackmap"),
            "no plain (unsound) stack map may remain:\n{output}"
        );
    }

    #[test]
    fn statepoint_mode_does_not_map_non_allocating_llvm_intrinsics() {
        let input = r#"define void @probe(i64 %arg, i1 %condition) {
entry.0:
  %r0 = alloca i64
  store i64 %arg, ptr %r0
  call void @js_shadow_slot_bind(i32 0, ptr %r0)
  call void @llvm.assume(i1 %condition)
  call void @may_collect()
  ret void
}
"#;
        let output = lower_statepoints(input, 1);
        assert!(output.contains("call void @llvm.assume(i1 %condition)"));
        assert_eq!(
            output
                .matches("@llvm.experimental.gc.statepoint.p0")
                .count(),
            1
        );
        assert!(!output.contains("@llvm.experimental.stackmap"));
    }

    #[test]
    fn audited_non_collecting_helpers_are_not_safepoints_in_either_backend() {
        let input = r#"define void @probe(i64 %arg) {
entry.0:
  %r0 = alloca i64
  store i64 %arg, ptr %r0
  call void @js_shadow_slot_bind(i32 0, ptr %r0)
  call void @js_gc_temp_root_push(i64 %arg)
  call void @js_write_barrier_root_nanbox(i64 %arg)
  call void @js_gc_loop_safepoint()
  ret void
}
"#;
        for output in [lower_statepoints(input, 1)] {
            assert!(output.contains("call void @js_gc_temp_root_push(i64 %arg)"));
            assert!(output.contains("call void @js_write_barrier_root_nanbox(i64 %arg)"));
            assert_eq!(
                output.matches("@llvm.experimental.stackmap").count()
                    + output
                        .matches("@llvm.experimental.gc.statepoint.p0")
                        .count(),
                1,
                "only the explicit collection boundary should be a safepoint:\n{output}"
            );
        }
    }
}
