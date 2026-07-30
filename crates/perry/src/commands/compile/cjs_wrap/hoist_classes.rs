//! Top-level `class` hoisting and `module.exports = class …` rewrite passes.

/// Issue #665 (fifth pass): rewrite the leaf-file shape
/// `module.exports = class Name { ... };` into declaration form
/// `class Name { ... }\nmodule.exports = Name;` so the existing
/// `extract_top_level_class_decls` + `extract_single_module_exports_assignment`
/// pipeline can surface the class as a module-scope binding. Returns the
/// rewritten source on success; `None` when the input does not match the
/// pattern (rest of the pipeline runs unchanged in that case).
///
/// This is the class-expression counterpart to the v0.5.839 fix, which
/// only handled the declaration form. Real-world packages like
/// rate-limiter-flexible (`lib/RateLimiterAbstract.js`) ship the
/// expression form, which made `super(opts)` calls from child classes
/// silently no-op the parent constructor — the consumer's `import X` saw
/// only the opaque `_cjs` IIFE result, never registered class identity
/// in compile.rs, and codegen's super-call dispatch fell through to the
/// no-parent-in-ctx branch.
///
/// Defensive constraints (returns `None` if any fails):
///   - exactly one top-level `module.exports = ...` assignment exists
///   - that assignment is anchored at column 0 (no leading whitespace)
///   - the RHS starts with `class\b`
///   - the class body is brace-balanced (with string/template/comment skip)
///   - the chosen class name does not collide with any existing top-level
///     `class <Name>` declaration in the source
pub fn rewrite_module_exports_class_expression(source: &str) -> Option<String> {
    // Find every `module.exports = ...` assignment at column 0. Multiple
    // (possibly conflicting) targets disqualify the rewrite — the IIFE's
    // last-assignment-wins semantics must keep running through `_cjs`.
    let any_assign_re = regex::Regex::new(r#"(?m)^module\.exports[\t ]*="#).ok()?;
    let assigns: Vec<_> = any_assign_re.find_iter(source).collect();
    if assigns.len() != 1 {
        return None;
    }
    let assign = &assigns[0];
    let assign_start = assign.start();
    let assign_end_byte = assign.end();

    let bytes = source.as_bytes();

    // Locate the `class` keyword after `module.exports =` (with optional
    // intervening spaces / tabs — we don't cross newlines into the RHS).
    let mut p = assign_end_byte;
    while p < bytes.len() && (bytes[p] == b' ' || bytes[p] == b'\t') {
        p += 1;
    }
    let class_kw_start = p;
    if class_kw_start + "class".len() > bytes.len() {
        return None;
    }
    if &bytes[class_kw_start..class_kw_start + "class".len()] != b"class" {
        return None;
    }
    // `class` must be followed by a non-identifier character (whitespace,
    // `{`, etc.) so we don't match `classify` or similar.
    let after_kw = class_kw_start + "class".len();
    if after_kw >= bytes.len() {
        return None;
    }
    let next = bytes[after_kw];
    let is_ident_cont = next.is_ascii_alphanumeric() || next == b'_' || next == b'$';
    if is_ident_cont {
        return None;
    }
    p = after_kw;

    // Skip whitespace (including newlines — the class body can span lines,
    // and the optional name may sit on the next line in rare formatting).
    while p < bytes.len() && bytes[p].is_ascii_whitespace() {
        p += 1;
    }

    // Optional class name.
    let name_start = p;
    while p < bytes.len()
        && (bytes[p].is_ascii_alphanumeric() || bytes[p] == b'_' || bytes[p] == b'$')
    {
        p += 1;
    }
    let name_end = p;
    let parsed_name = if name_end > name_start {
        Some(
            std::str::from_utf8(&bytes[name_start..name_end])
                .ok()?
                .to_string(),
        )
    } else {
        None
    };

    // Scan forward to the opening `{` of the class body. `extends X`
    // clauses live here and may include member access / call expressions,
    // but not newlines that exit the declaration head — class bodies
    // always open with `{` before any executable statement.
    while p < bytes.len() && bytes[p] != b'{' {
        p += 1;
    }
    if p >= bytes.len() {
        return None;
    }
    let body_start = p;

    // Brace-balanced scan, skipping string / template / line-comment /
    // block-comment contents. Mirrors the logic in
    // `extract_top_level_class_decls`.
    let mut depth: i32 = 0;
    let mut r = body_start;
    while r < bytes.len() {
        match bytes[r] {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    r += 1;
                    break;
                }
            }
            b'"' | b'\'' => {
                let quote = bytes[r];
                r += 1;
                while r < bytes.len() && bytes[r] != quote {
                    if bytes[r] == b'\\' && r + 1 < bytes.len() {
                        r += 2;
                        continue;
                    }
                    r += 1;
                }
            }
            b'`' => {
                r += 1;
                while r < bytes.len() && bytes[r] != b'`' {
                    if bytes[r] == b'\\' && r + 1 < bytes.len() {
                        r += 2;
                        continue;
                    }
                    r += 1;
                }
            }
            b'/' if r + 1 < bytes.len() && bytes[r + 1] == b'/' => {
                r += 2;
                while r < bytes.len() && bytes[r] != b'\n' {
                    r += 1;
                }
            }
            b'/' if r + 1 < bytes.len() && bytes[r + 1] == b'*' => {
                r += 2;
                while r + 1 < bytes.len() && !(bytes[r] == b'*' && bytes[r + 1] == b'/') {
                    r += 1;
                }
                if r + 1 < bytes.len() {
                    r += 2;
                }
            }
            _ => {}
        }
        r += 1;
    }
    if depth != 0 {
        return None;
    }
    let body_end = r;

    // Optional trailing whitespace + optional `;` to consume.
    let mut q = body_end;
    while q < bytes.len() && (bytes[q] == b' ' || bytes[q] == b'\t') {
        q += 1;
    }
    if q < bytes.len() && bytes[q] == b';' {
        q += 1;
    }

    // Pick the name to use in the rewritten declaration. Anonymous gets
    // a synthetic name. Reject if a top-level `class <ChosenName>`
    // declaration already exists — we don't want to emit duplicates.
    let chosen_name = parsed_name
        .clone()
        .unwrap_or_else(|| "__perry_cjs_default__".to_string());
    let collision_pattern = format!(r#"(?m)^class[\t ]+{}\b"#, regex::escape(&chosen_name));
    let collision_re = regex::Regex::new(&collision_pattern).ok()?;
    if collision_re.is_match(source) {
        return None;
    }

    // Build the replacement. Use the original class head when named
    // (`class Foo extends Bar `) so any extends clause survives byte-for-byte.
    // For anonymous, inject the synthetic name between `class` and the rest.
    let class_head = if parsed_name.is_some() {
        std::str::from_utf8(&bytes[class_kw_start..body_start])
            .ok()?
            .to_string()
    } else {
        let after_class_kw = std::str::from_utf8(&bytes[after_kw..body_start]).ok()?;
        format!("class {}{}", chosen_name, after_class_kw)
    };
    let class_body = std::str::from_utf8(&bytes[body_start..body_end]).ok()?;
    let replacement = format!(
        "{}{}\nmodule.exports = {};",
        class_head, class_body, chosen_name
    );

    let mut s = source.to_string();
    s.replace_range(assign_start..q, &replacement);
    Some(s)
}

/// Issue #652: extract top-level `class X { ... }` declarations from the CJS
/// source so they can be hoisted OUT of the wrapping IIFE. Returns:
///   - the extracted class block (joined with newlines, empty if none)
///   - the list of class names extracted
///   - the source with the class blocks replaced by blank lines (preserves
///     line numbers for diagnostics)
///
/// Detection is brace-balanced, anchored to lines where `class ` appears at
/// column 0 (strict top-level only — nested classes inside functions /
/// blocks / object literals are left alone). Skips classes whose name is
/// already a duplicate of a previously-seen class (defensive).
pub fn extract_top_level_class_decls(source: &str) -> (String, Vec<String>, String) {
    let bytes = source.as_bytes();

    // A top-level `class` candidate discovered by the brace-balanced scan.
    // We collect every candidate first, then decide hoist-vs-keep in a
    // chain-aware fixpoint pass below — deciding per class independently
    // (the original single forward pass) splits an inheritance chain across
    // the IIFE boundary: a parent KEPT inside the IIFE (because it refs an
    // IIFE-local) becomes invisible to a child HOISTED to top level, so the
    // child's `extends <Parent>` throws `ReferenceError: <Parent> is not
    // defined` (ajv `class AssignOp extends Assign` / `new Assign(...)`).
    struct ClassCand<'a> {
        name: String,
        block_text: &'a str,
        // `extends <Parent>` parent identifier (first ident after `extends`),
        // if this class extends a bare same-module name (not a member access
        // like `_x.default` — those resolve via a different path).
        extends_parent: Option<String>,
        line_start: usize,
        end: usize,
        // True iff the class body / extends-head textually references an
        // IIFE-local binding (#2310 / #5251). Such a class genuinely cannot
        // be hoisted out of the IIFE.
        references_iife_local: bool,
    }
    let mut candidates: Vec<ClassCand> = Vec::new();

    // Issue #2310 — collect the names of top-level `let`/`const`/`var`
    // declarations in this CJS source so we can REFUSE to hoist a class
    // whose body references any of them. Hoisting moves the class out of
    // the IIFE wrap, which severs the closure over those bindings — the
    // class's methods then see `unknown identifier` warnings and any
    // `++`/`--` update on an IIFE-internal let hard-errors with
    // `Undefined variable in update expression`. The conservative rule
    // is to leave the class inside the IIFE when *any* of its referenced
    // identifiers would lose their binding to the IIFE-local state.
    let mut iife_locals = collect_top_level_let_const_var_names(source);
    // Issue #5251 — the cjs_wrap preamble injects `var exports`, `var module`,
    // and a `require` function as IIFE-local bindings (see `wrap.rs`'s
    // `cjs_preamble`). They are NOT declared in the original source, so the
    // textual top-level scan above never sees them. A class whose body reads
    // `exports.X` / `module.exports` / `require(...)` must therefore ALSO stay
    // inside the IIFE — hoisting it out severs the closure over the injected
    // `var exports`, and `exports` then resolves as an unknown global
    // (`exports.X` lowers to the numeric `0` sentinel → `(number).test is not a
    // function` inside class methods/constructors of CJS packages like ajv).
    for injected in ["exports", "module", "require"] {
        if !iife_locals.iter().any(|n| n == injected) {
            iife_locals.push(injected.to_string());
        }
    }
    // #6585: function declarations are IIFE-local too. Ajv declares
    // `function addNames(...)` after several classes whose getters call it.
    // Hoisting those classes out of the CJS IIFE severed the function binding,
    // so the getters fell through to a global read and threw
    // `ReferenceError: addNames is not defined`.
    //
    // Preserve class identity whenever possible: a capture-free helper can be
    // duplicated at module scope (the original stays in the IIFE), allowing
    // the class to keep using the established hoist path. A helper that reads
    // any IIFE binding, sibling helper, or class cannot be duplicated safely;
    // add only those unsafe names to `iife_locals`, which keeps their dependent
    // classes inside the wrapper.
    let stripped_source = super::detect::strip_comments_and_strings(source);
    let function_decls = collect_top_level_function_decls(source, &stripped_source);
    let function_names: Vec<String> = function_decls.iter().map(|f| f.name.clone()).collect();
    let class_names = top_level_class_names(source);
    let exported_names = super::extract_exports::extract_exports_from_source(source);
    let mut unsafe_function_names = Vec::new();
    for decl in &function_decls {
        let mut blockers = iife_locals.clone();
        blockers.extend(class_names.iter().cloned());
        blockers.extend(
            function_names
                .iter()
                .filter(|name| *name != &decl.name)
                .cloned(),
        );
        let unsafe_to_duplicate = class_body_references_any(&decl.block_text, &blockers)
            || super::extract_requires::identifier_is_reassigned(source, &decl.name)
            || identifier_used_as_value_outside(
                &stripped_source,
                &decl.name,
                decl.source_start,
                decl.source_end,
            )
            || exported_names.contains(&decl.name);
        if unsafe_to_duplicate {
            unsafe_function_names.push(decl.name.clone());
            if !iife_locals.contains(&decl.name) {
                iife_locals.push(decl.name.clone());
            }
        }
    }

    let mut i = 0usize;
    while i < bytes.len() {
        // Anchor on a `class` keyword at the start of a line (after only
        // whitespace would also be acceptable in principle, but real CJS
        // packages put their class declarations at column 0).
        let line_start = if i == 0 || bytes[i - 1] == b'\n' {
            i
        } else {
            // Find the next newline; advance.
            i += 1;
            continue;
        };

        // Column-0 only: an indented `class` is (almost always) nested inside
        // a function — `function mod() {\n  const f = ...;\n  class Event2 {
        // constructor(t) { this[f] = t; } }\n}` (the `ws` package's event
        // classes have this shape). Hoisting a nested class out of the IIFE
        // severs its closure over the enclosing function's locals, turning
        // `f` into a ReferenceError at runtime. The #2310 let/const/var
        // guard below can't catch those — it only collects TOP-LEVEL names.
        let p = line_start;

        if p + 6 <= bytes.len() && &bytes[p..p + 6] == b"class " {
            // Skip past "class ".
            let name_start = p + 6;
            // Scan identifier.
            let mut name_end = name_start;
            while name_end < bytes.len() {
                let c = bytes[name_end];
                let valid = (c.is_ascii_alphanumeric()) || c == b'_' || c == b'$';
                if !valid {
                    break;
                }
                name_end += 1;
            }
            if name_end > name_start {
                let class_name = std::str::from_utf8(&bytes[name_start..name_end])
                    .unwrap_or("")
                    .to_string();
                // Skip whitespace + optional `extends ...` clause + opening `{`.
                let mut q = name_end;
                while q < bytes.len() && (bytes[q] == b' ' || bytes[q] == b'\t') {
                    q += 1;
                }
                // Optional `extends X` (or `extends X.Y` / `extends X(arg)` etc.) — scan
                // until we hit the opening `{` for the class body, refusing
                // to cross newlines so we stay inside the declaration head.
                while q < bytes.len() && bytes[q] != b'{' && bytes[q] != b'\n' {
                    q += 1;
                }
                if q < bytes.len() && bytes[q] == b'{' {
                    // Brace-balanced scan to find the matching closing `}`.
                    let body_start = q;
                    let mut depth: i32 = 0;
                    let mut r = q;
                    while r < bytes.len() {
                        match bytes[r] {
                            b'{' => depth += 1,
                            b'}' => {
                                depth -= 1;
                                if depth == 0 {
                                    r += 1;
                                    break;
                                }
                            }
                            // String / template / line-comment / block-comment
                            // skip — minimal handling, sufficient for typical
                            // class bodies. Class bodies don't usually contain
                            // string literals with stray braces, but handle
                            // the common cases defensively.
                            b'"' | b'\'' => {
                                let quote = bytes[r];
                                r += 1;
                                while r < bytes.len() && bytes[r] != quote {
                                    if bytes[r] == b'\\' && r + 1 < bytes.len() {
                                        r += 2;
                                        continue;
                                    }
                                    r += 1;
                                }
                            }
                            b'`' => {
                                r += 1;
                                while r < bytes.len() && bytes[r] != b'`' {
                                    if bytes[r] == b'\\' && r + 1 < bytes.len() {
                                        r += 2;
                                        continue;
                                    }
                                    r += 1;
                                }
                            }
                            b'/' if r + 1 < bytes.len() && bytes[r + 1] == b'/' => {
                                r += 2;
                                while r < bytes.len() && bytes[r] != b'\n' {
                                    r += 1;
                                }
                            }
                            b'/' if r + 1 < bytes.len() && bytes[r + 1] == b'*' => {
                                r += 2;
                                while r + 1 < bytes.len()
                                    && !(bytes[r] == b'*' && bytes[r + 1] == b'/')
                                {
                                    r += 1;
                                }
                                if r + 1 < bytes.len() {
                                    r += 2;
                                }
                            }
                            _ => {}
                        }
                        r += 1;
                    }
                    if depth == 0 && r > body_start {
                        // Successful brace-balanced match. Record the block —
                        // unless the body references an IIFE-local let/const/var,
                        // in which case hoisting would sever the closure (#2310).
                        let block_text = std::str::from_utf8(&bytes[line_start..r]).unwrap_or("");
                        let body_text = std::str::from_utf8(&bytes[body_start..r]).unwrap_or("");
                        // The `extends` clause head (between the class name and the
                        // body's opening `{`) can reference an IIFE-local require
                        // alias, e.g. `class Derived extends _suffix.default { ... }`
                        // (the Next.js `NextNodeServer extends base-server.default`
                        // interop pattern). Hoisting the class above its
                        // `const _suffix = _interop(require(...))` evaluates the
                        // parent before the alias is assigned, so the dynamic
                        // parent-registration sees `undefined` and throws "Class
                        // extends value is not a constructor". Treat an extends-head
                        // reference to an IIFE-local the same as a body reference:
                        // keep the class inside the IIFE at its source position.
                        let extends_head =
                            std::str::from_utf8(&bytes[name_end..body_start]).unwrap_or("");
                        let references_iife_local =
                            class_body_references_any(body_text, &iife_locals)
                                || class_body_references_any(extends_head, &iife_locals);
                        let extends_parent = parse_extends_parent(extends_head);
                        if !candidates.iter().any(|c| c.name == class_name) {
                            candidates.push(ClassCand {
                                name: class_name,
                                block_text,
                                extends_parent,
                                line_start,
                                end: r,
                                references_iife_local,
                            });
                        }
                        i = r;
                        continue;
                    }
                }
            }
        }
        // Advance to the next line.
        while i < bytes.len() && bytes[i] != b'\n' {
            i += 1;
        }
        i += 1;
    }

    // Chain-aware keep decision. Start by keeping every class that directly
    // references an IIFE-local; then iterate to a fixpoint, additionally
    // keeping any class whose `extends` parent is a same-module sibling that
    // is itself kept. A kept parent cannot be hoisted (it's an IIFE-local),
    // so a hoisted child would lose visibility of it — keep the child too.
    // Parents resolved via member access / require alias are handled by the
    // #2310/#5251 textual checks already and are not in `candidates`, so they
    // never force-keep here.
    let cand_names: std::collections::HashSet<&str> =
        candidates.iter().map(|c| c.name.as_str()).collect();
    let mut kept: Vec<bool> = candidates.iter().map(|c| c.references_iife_local).collect();
    loop {
        let mut changed = false;
        let kept_names: Vec<String> = candidates
            .iter()
            .enumerate()
            .filter(|(idx, _)| kept[*idx])
            .map(|(_, cand)| cand.name.clone())
            .collect();
        for idx in 0..candidates.len() {
            if kept[idx] {
                continue;
            }
            if let Some(parent) = &candidates[idx].extends_parent {
                // Only same-module sibling parents matter.
                if cand_names.contains(parent.as_str()) {
                    let parent_kept = candidates
                        .iter()
                        .position(|c| &c.name == parent)
                        .map(|pidx| kept[pidx])
                        .unwrap_or(false);
                    if parent_kept {
                        kept[idx] = true;
                        changed = true;
                    }
                }
            }
            // A class can depend on a kept sibling without extending it
            // (`CodeGen.run()` does `new ParentNode()`). Once one class must
            // stay in the IIFE, keep every class whose body references it,
            // iterating to a fixpoint for longer dependency chains.
            if !kept[idx] && class_body_references_any(candidates[idx].block_text, &kept_names) {
                kept[idx] = true;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }

    let mut hoisted_blocks: Vec<String> = function_decls
        .iter()
        .filter(|decl| !unsafe_function_names.contains(&decl.name))
        .filter(|decl| {
            candidates.iter().enumerate().any(|(idx, cand)| {
                !kept[idx]
                    && class_body_references_any(cand.block_text, std::slice::from_ref(&decl.name))
            })
        })
        .map(|decl| decl.block_text.clone())
        .collect();
    let mut hoisted_names: Vec<String> = Vec::new();
    let mut elided: Vec<(usize, usize)> = Vec::new();
    for (idx, cand) in candidates.iter().enumerate() {
        if kept[idx] {
            continue;
        }
        hoisted_blocks.push(cand.block_text.to_string());
        hoisted_names.push(cand.name.clone());
        elided.push((cand.line_start, cand.end));
    }

    let mut out_source = source.to_string();
    // Replace the elided ranges with whitespace (back-to-front to preserve
    // earlier indices). Empty out the original class body but keep newlines
    // for line-number stability.
    for (start, end) in elided.iter().rev() {
        let original = &source[*start..*end];
        let blanked: String = original
            .chars()
            .map(|c| if c == '\n' { '\n' } else { ' ' })
            .collect();
        out_source.replace_range(*start..*end, &blanked);
    }

    let hoisted_block = hoisted_blocks.join("\n");
    (hoisted_block, hoisted_names, out_source)
}

/// Issue #2310 — scan the source for **top-level** `let`/`const`/`var`
/// declarations and return their bare identifier names. Used by
/// `extract_top_level_class_decls` to refuse a hoist when the candidate
/// class body references any of these — the wrap moves the class out of
/// the IIFE so a hoisted class can't reach an IIFE-local binding, which
/// surfaces as `Undefined variable in update expression` for `x++`/`x--`
/// inside the class body.
///
/// Detection is intentionally textual (no SWC parse): the wrap layer
/// already operates on raw source ranges, and the cost of a parse here
/// is wasted work for the >95% of CJS files that don't trip this case.
/// We accept the same anchor rule as the class scan: declarations at the
/// start of a line (after only whitespace), brace-depth-aware so we
/// ignore decls inside functions / classes / object literals.
/// Walk a brace/bracket-balanced binding pattern starting at `start`
/// (which must point at the opening `{` or `[`) and push every bound
/// identifier into `names`. Returns the byte index just past the
/// matching closing delimiter.
///
/// Object patterns bind the *target* of each `key: target` pair (the
/// shorthand `key` when no `: target` is present); array patterns bind
/// each element. Defaults (`= expr`) and computed/literal keys are
/// skipped over so they don't contribute spurious names, and a rest
/// element (`...rest`) binds `rest`. Nested patterns recurse. This is a
/// conservative textual walk (no SWC parse) matching the rest of the
/// cjs_wrap layer; over-collecting a name is harmless here (it only
/// makes the #2310 hoist guard *more* conservative), so the goal is to
/// never MISS a real binding.
fn collect_pattern_binding_names(bytes: &[u8], start: usize, names: &mut Vec<String>) -> usize {
    let open = bytes[start];
    let (close, is_object) = if open == b'{' {
        (b'}', true)
    } else {
        (b']', false)
    };
    let mut i = start + 1;
    // In an object pattern, after a `:` the following identifier is the
    // binding target (not a key); track that so `{ key: target }` records
    // `target`, while a bare `{ key }` records `key`.
    let mut after_colon = false;
    while i < bytes.len() {
        match bytes[i] {
            b'{' | b'[' => {
                i = collect_pattern_binding_names(bytes, i, names);
                after_colon = false;
                continue;
            }
            c if c == close => {
                i += 1;
                break;
            }
            // Skip string / template keys.
            b'"' | b'\'' => {
                let q = bytes[i];
                i += 1;
                while i < bytes.len() && bytes[i] != q {
                    if bytes[i] == b'\\' && i + 1 < bytes.len() {
                        i += 2;
                        continue;
                    }
                    i += 1;
                }
                i += 1;
            }
            b'`' => {
                i += 1;
                while i < bytes.len() && bytes[i] != b'`' {
                    if bytes[i] == b'\\' && i + 1 < bytes.len() {
                        i += 2;
                        continue;
                    }
                    i += 1;
                }
                i += 1;
            }
            // A default initializer (`= expr`) — skip to the next top-level
            // comma or the pattern's closing delimiter, brace/paren aware so
            // a default object/array/call doesn't confuse the element split.
            b'=' if i + 1 >= bytes.len() || bytes[i + 1] != b'=' => {
                i += 1;
                let mut inner: i32 = 0;
                while i < bytes.len() {
                    match bytes[i] {
                        b'(' | b'[' | b'{' => inner += 1,
                        b')' | b']' | b'}' if inner > 0 => inner -= 1,
                        c if c == close && inner == 0 => break,
                        b',' if inner == 0 => break,
                        b'"' | b'\'' => {
                            let q = bytes[i];
                            i += 1;
                            while i < bytes.len() && bytes[i] != q {
                                if bytes[i] == b'\\' && i + 1 < bytes.len() {
                                    i += 2;
                                    continue;
                                }
                                i += 1;
                            }
                        }
                        _ => {}
                    }
                    i += 1;
                }
                after_colon = false;
            }
            b':' => {
                after_colon = true;
                i += 1;
            }
            b',' => {
                after_colon = false;
                i += 1;
            }
            c if c.is_ascii_alphanumeric() || c == b'_' || c == b'$' || c == b'.' => {
                // Identifier (or `...rest` after the dots). Read it.
                let name_start = i;
                while i < bytes.len()
                    && (bytes[i].is_ascii_alphanumeric()
                        || bytes[i] == b'_'
                        || bytes[i] == b'$'
                        || bytes[i] == b'.')
                {
                    i += 1;
                }
                let raw = std::str::from_utf8(&bytes[name_start..i]).unwrap_or("");
                // Strip a leading `...` (rest element).
                let ident = raw.trim_start_matches('.');
                // In an object pattern, a bare identifier followed (after
                // whitespace) by `:` is a KEY, not a binding — defer to the
                // post-colon identifier. Peek ahead past whitespace.
                let mut j = i;
                while j < bytes.len() && (bytes[j] == b' ' || bytes[j] == b'\t') {
                    j += 1;
                }
                let is_key = is_object && !after_colon && j < bytes.len() && bytes[j] == b':';
                if !is_key && !ident.is_empty() && !ident.contains('.') {
                    let n = ident.to_string();
                    if !names.contains(&n) {
                        names.push(n);
                    }
                }
                after_colon = false;
            }
            _ => {
                i += 1;
            }
        }
    }
    i
}

fn collect_top_level_let_const_var_names(source: &str) -> Vec<String> {
    let bytes = source.as_bytes();
    let mut names: Vec<String> = Vec::new();
    let mut depth: i32 = 0;
    let mut i = 0usize;

    while i < bytes.len() {
        let at_line_start = i == 0 || bytes[i - 1] == b'\n';
        // Update brace depth for non-line-start bytes (and also at line start —
        // a line starting with `}` decreases depth before the keyword scan).
        match bytes[i] {
            b'{' => {
                depth += 1;
                i += 1;
                continue;
            }
            b'}' => {
                depth -= 1;
                i += 1;
                continue;
            }
            b'"' | b'\'' => {
                let q = bytes[i];
                i += 1;
                while i < bytes.len() && bytes[i] != q {
                    if bytes[i] == b'\\' && i + 1 < bytes.len() {
                        i += 2;
                        continue;
                    }
                    i += 1;
                }
                i += 1;
                continue;
            }
            b'`' => {
                i += 1;
                while i < bytes.len() && bytes[i] != b'`' {
                    if bytes[i] == b'\\' && i + 1 < bytes.len() {
                        i += 2;
                        continue;
                    }
                    i += 1;
                }
                i += 1;
                continue;
            }
            b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'/' => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
                continue;
            }
            b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'*' => {
                i += 2;
                while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                    i += 1;
                }
                if i + 1 < bytes.len() {
                    i += 2;
                }
                continue;
            }
            _ => {}
        }
        if !at_line_start || depth != 0 {
            i += 1;
            continue;
        }
        // At column 0 of a line, depth 0: probe for let/const/var (optionally
        // preceded by whitespace, though real CJS rarely indents top-level
        // decls).
        let mut p = i;
        while p < bytes.len() && (bytes[p] == b' ' || bytes[p] == b'\t') {
            p += 1;
        }
        let rest = &bytes[p..];
        let kw_len = if rest.starts_with(b"const ") || rest.starts_with(b"const\t") {
            6
        } else if rest.starts_with(b"let ") || rest.starts_with(b"let\t") {
            4
        } else if rest.starts_with(b"var ") || rest.starts_with(b"var\t") {
            4
        } else {
            i += 1;
            continue;
        };
        let mut q = p + kw_len;
        // Walk through one or more comma-separated declarators on the same
        // logical line. Stop at `=` (initializer), `;`, or the end of line
        // (semicolon optional in JS).
        loop {
            while q < bytes.len() && (bytes[q] == b' ' || bytes[q] == b'\t') {
                q += 1;
            }
            // Issue: a destructuring declarator (`const { tbl } = require(...)`,
            // `const [a, b] = ...`) binds names INSIDE a `{…}`/`[…]` pattern,
            // not as a bare identifier. The plain identifier scan below skips
            // straight past the `{`/`[` and captures nothing, so a class whose
            // body references `tbl` looks free of IIFE-locals → gets hoisted →
            // severs its closure over `tbl` → `tbl` resolves to the `_cjs.tbl`
            // export read-back, which is `undefined` (semver's `const { t } =
            // require('./re')` read from a class method is exactly this). Walk
            // the brace/bracket-balanced pattern and collect every bound name.
            if q < bytes.len() && (bytes[q] == b'{' || bytes[q] == b'[') {
                let pat_end = collect_pattern_binding_names(bytes, q, &mut names);
                q = pat_end;
            } else {
                let name_start = q;
                while q < bytes.len()
                    && (bytes[q].is_ascii_alphanumeric() || bytes[q] == b'_' || bytes[q] == b'$')
                {
                    q += 1;
                }
                if q > name_start {
                    let n = std::str::from_utf8(&bytes[name_start..q])
                        .unwrap_or("")
                        .to_string();
                    if !n.is_empty() && !names.contains(&n) {
                        names.push(n);
                    }
                }
            }
            // Skip ahead past an `=` initializer (brace/paren/bracket-aware so a
            // `let m = { … }` doesn't trip our depth tracking) to the next
            // comma or end of statement.
            let mut inner: i32 = 0;
            while q < bytes.len() {
                match bytes[q] {
                    b'(' | b'[' | b'{' => inner += 1,
                    b')' | b']' | b'}' => inner -= 1,
                    b'"' | b'\'' => {
                        let qq = bytes[q];
                        q += 1;
                        while q < bytes.len() && bytes[q] != qq {
                            if bytes[q] == b'\\' && q + 1 < bytes.len() {
                                q += 2;
                                continue;
                            }
                            q += 1;
                        }
                    }
                    b'`' => {
                        q += 1;
                        while q < bytes.len() && bytes[q] != b'`' {
                            if bytes[q] == b'\\' && q + 1 < bytes.len() {
                                q += 2;
                                continue;
                            }
                            q += 1;
                        }
                    }
                    b',' if inner == 0 => {
                        q += 1;
                        break;
                    }
                    b';' | b'\n' if inner == 0 => {
                        // End of declaration.
                        i = q;
                        break;
                    }
                    _ => {}
                }
                q += 1;
            }
            if q >= bytes.len() || bytes[q] == b';' || bytes[q] == b'\n' {
                break;
            }
        }
        // Advance past the line; the outer loop will keep scanning.
        while i < bytes.len() && bytes[i] != b'\n' {
            i += 1;
        }
        i += 1;
    }

    names
}

struct TopLevelFunctionDecl {
    name: String,
    block_text: String,
    source_start: usize,
    source_end: usize,
}

/// Collect depth-zero function declarations from a CommonJS source, including
/// their exact source blocks. The comment/string/regex masker preserves byte
/// positions, letting this scan balance braces without mistaking literal
/// contents for code.
fn collect_top_level_function_decls(
    source: &str,
    stripped_source: &str,
) -> Vec<TopLevelFunctionDecl> {
    fn is_ident_byte(b: u8) -> bool {
        b == b'_' || b == b'$' || b.is_ascii_alphanumeric()
    }
    fn skip_horizontal_space(bytes: &[u8], mut i: usize) -> usize {
        while bytes.get(i).is_some_and(|b| matches!(b, b' ' | b'\t')) {
            i += 1;
        }
        i
    }

    let bytes = stripped_source.as_bytes();
    let mut decls = Vec::new();
    let mut depth: i32 = 0;
    let mut i = 0usize;
    while i < bytes.len() {
        let at_line_start = i == 0 || bytes[i - 1] == b'\n';
        match bytes[i] {
            b'{' => {
                depth += 1;
                i += 1;
                continue;
            }
            b'}' => {
                depth -= 1;
                i += 1;
                continue;
            }
            _ => {}
        }
        if !at_line_start || depth != 0 {
            i += 1;
            continue;
        }

        let mut cursor = skip_horizontal_space(bytes, i);
        if bytes.get(cursor..cursor + 5) == Some(b"async")
            && bytes
                .get(cursor + 5)
                .is_some_and(|b| matches!(b, b' ' | b'\t'))
        {
            cursor = skip_horizontal_space(bytes, cursor + 5);
        }
        if bytes.get(cursor..cursor + 8) != Some(b"function")
            || bytes.get(cursor + 8).is_some_and(|b| is_ident_byte(*b))
        {
            i += 1;
            continue;
        }
        cursor = skip_horizontal_space(bytes, cursor + 8);
        if bytes.get(cursor) == Some(&b'*') {
            cursor = skip_horizontal_space(bytes, cursor + 1);
        }
        let start = cursor;
        while bytes.get(cursor).is_some_and(|b| is_ident_byte(*b)) {
            cursor += 1;
        }
        if cursor == start {
            i += 1;
            continue;
        }
        let name = String::from_utf8_lossy(&bytes[start..cursor]).into_owned();

        // Find the body opener, ignoring braces in destructured/default
        // parameters while the parameter list is still open.
        let mut paren_depth: i32 = 0;
        let mut body_start = None;
        while cursor < bytes.len() {
            match bytes[cursor] {
                b'(' | b'[' => paren_depth += 1,
                b')' | b']' => paren_depth -= 1,
                b'{' if paren_depth == 0 => {
                    body_start = Some(cursor);
                    break;
                }
                _ => {}
            }
            cursor += 1;
        }
        let Some(body_start) = body_start else {
            i += 1;
            continue;
        };

        let mut body_depth: i32 = 0;
        let mut end = body_start;
        while end < bytes.len() {
            match bytes[end] {
                b'{' => body_depth += 1,
                b'}' => {
                    body_depth -= 1;
                    if body_depth == 0 {
                        end += 1;
                        break;
                    }
                }
                _ => {}
            }
            end += 1;
        }
        if body_depth != 0 || end <= body_start {
            i += 1;
            continue;
        }

        if !name.is_empty()
            && !decls
                .iter()
                .any(|decl: &TopLevelFunctionDecl| decl.name == name)
        {
            decls.push(TopLevelFunctionDecl {
                name,
                block_text: source[i..end].to_string(),
                source_start: i,
                source_end: end,
            });
        }
        i = end;
    }
    decls
}

/// Returns true when a function declaration's binding is used as a value
/// elsewhere in the module rather than only called directly.
///
/// Duplicating a capture-free helper is safe for `helper(...)` calls, but not
/// when the IIFE mutates the original function object (`helper.cache = ...`),
/// aliases it (`const cb = helper`), compares its identity, or passes it to
/// another function. In those cases a module-scope duplicate would diverge
/// from the original. Member property names such as `obj.helper()` are
/// unrelated bindings and are ignored.
fn identifier_used_as_value_outside(
    stripped_source: &str,
    name: &str,
    excluded_start: usize,
    excluded_end: usize,
) -> bool {
    fn is_ident_byte(b: u8) -> bool {
        b == b'_' || b == b'$' || b.is_ascii_alphanumeric()
    }

    let bytes = stripped_source.as_bytes();
    let name_bytes = name.as_bytes();
    let mut i = 0usize;
    while i + name_bytes.len() <= bytes.len() {
        if i >= excluded_start && i < excluded_end && excluded_end <= bytes.len() {
            i = excluded_end;
            continue;
        }
        if &bytes[i..i + name_bytes.len()] != name_bytes {
            i += 1;
            continue;
        }

        let before_ok = i == 0 || !is_ident_byte(bytes[i - 1]);
        let after = i + name_bytes.len();
        let after_ok = after >= bytes.len() || !is_ident_byte(bytes[after]);
        if !before_ok || !after_ok {
            i = after;
            continue;
        }

        let mut before = i;
        while before > 0 && bytes[before - 1].is_ascii_whitespace() {
            before -= 1;
        }
        // `obj.helper` / `obj?.helper` names a property, not this binding.
        if before > 0 && bytes[before - 1] == b'.' {
            i = after;
            continue;
        }

        let mut cursor = after;
        while bytes.get(cursor).is_some_and(|b| b.is_ascii_whitespace()) {
            cursor += 1;
        }
        if bytes.get(cursor) != Some(&b'(') {
            return true;
        }
        i = cursor + 1;
    }
    false
}

/// Issue #4933 — collect the names of every **top-level** `class <Name>`
/// declaration anchored at column 0, regardless of whether it would hoist.
/// `extract_top_level_class_decls` only returns the classes it actually
/// hoists (it refuses any whose body references an IIFE-local binding,
/// #2310), so a `module.exports = StackUtils` whose `StackUtils` reads a
/// top-level `const natives = …` is invisible to the hoisted-name list.
/// The flat-emit path (wrap.rs) needs to know the assignment target is a
/// real top-level class before it drops the IIFE, hence this companion
/// scan. Uses the same column-0 anchor + identifier rule as the hoist scan.
pub fn top_level_class_names(source: &str) -> Vec<String> {
    let bytes = source.as_bytes();
    let mut names: Vec<String> = Vec::new();
    let mut i = 0usize;
    while i < bytes.len() {
        let at_line_start = i == 0 || bytes[i - 1] == b'\n';
        if !at_line_start {
            i += 1;
            continue;
        }
        let mut p = i;
        while p < bytes.len() && (bytes[p] == b' ' || bytes[p] == b'\t') {
            p += 1;
        }
        if p + 6 <= bytes.len() && &bytes[p..p + 6] == b"class " {
            let name_start = p + 6;
            let mut name_end = name_start;
            while name_end < bytes.len() {
                let c = bytes[name_end];
                if !(c.is_ascii_alphanumeric() || c == b'_' || c == b'$') {
                    break;
                }
                name_end += 1;
            }
            if name_end > name_start {
                if let Ok(name) = std::str::from_utf8(&bytes[name_start..name_end]) {
                    if !name.is_empty() && !names.contains(&name.to_string()) {
                        names.push(name.to_string());
                    }
                }
            }
        }
        while i < bytes.len() && bytes[i] != b'\n' {
            i += 1;
        }
        i += 1;
    }
    names
}

/// Issue #4933 — true if the CJS body has a `return` statement at the very
/// top level (brace depth 0). The IIFE wrap turns the module body into a
/// function, so a top-level `return` (legal in a CommonJS module, where
/// Node wraps the body in a function) is valid there. The flat-emit path
/// drops the IIFE and runs the body at ESM module scope, where such a
/// `return` would change meaning — so we keep the IIFE for those modules.
/// Detection is brace-depth-aware with string/template/comment skipping,
/// mirroring `collect_top_level_let_const_var_names`. A braced top-level
/// return (`if (x) { return; }`) sits at depth ≥ 1 and is not caught here;
/// Perry already treats a module-scope `return` as a no-op rather than an
/// error, so the residual risk is a rare semantic nuance, not a miscompile.
pub fn source_has_top_level_return(source: &str) -> bool {
    let bytes = source.as_bytes();
    let mut depth: i32 = 0;
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            // Track only `{`/`}` — block / function / class bodies — like the
            // sibling `collect_top_level_let_const_var_names`. Counting `(`/`[`
            // too would let an un-skipped regex literal's brackets corrupt the
            // depth and mis-flag a function-body `return` as top-level (the
            // stack-utils `const methodRe = /…\[as…\]…/` false positive).
            b'{' => {
                depth += 1;
                i += 1;
                continue;
            }
            b'}' => {
                depth -= 1;
                i += 1;
                continue;
            }
            b'"' | b'\'' => {
                let q = bytes[i];
                i += 1;
                while i < bytes.len() && bytes[i] != q {
                    if bytes[i] == b'\\' && i + 1 < bytes.len() {
                        i += 2;
                        continue;
                    }
                    i += 1;
                }
                i += 1;
                continue;
            }
            b'`' => {
                i += 1;
                while i < bytes.len() && bytes[i] != b'`' {
                    if bytes[i] == b'\\' && i + 1 < bytes.len() {
                        i += 2;
                        continue;
                    }
                    i += 1;
                }
                i += 1;
                continue;
            }
            b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'/' => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
                continue;
            }
            b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'*' => {
                i += 2;
                while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                    i += 1;
                }
                if i + 1 < bytes.len() {
                    i += 2;
                }
                continue;
            }
            _ => {}
        }
        if depth == 0 && bytes[i] == b'r' && source[i..].starts_with("return") {
            let before_ok = i == 0
                || !(bytes[i - 1].is_ascii_alphanumeric()
                    || bytes[i - 1] == b'_'
                    || bytes[i - 1] == b'$'
                    || bytes[i - 1] == b'.');
            let after = i + "return".len();
            let after_ok = after >= bytes.len()
                || !(bytes[after].is_ascii_alphanumeric()
                    || bytes[after] == b'_'
                    || bytes[after] == b'$');
            if before_ok && after_ok {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// Parse the `extends` parent identifier out of a class head's extends-clause
/// text (the slice between the class name and the body's opening `{`, e.g.
/// ` extends Assign `). Returns the bare parent identifier ONLY when the
/// extends target is a simple same-module name — `Some("Assign")`. Returns
/// `None` for an absent `extends`, or for a member-access / call-expression
/// parent (`extends _code.default`, `extends mixin(Base)`) since those
/// resolve via the require-alias path, not as a sibling top-level class.
fn parse_extends_parent(extends_head: &str) -> Option<String> {
    let trimmed = extends_head.trim_start();
    let rest = trimmed.strip_prefix("extends")?;
    // `extends` must be followed by whitespace (not `extendsX`).
    let after = rest.strip_prefix(|c: char| c.is_ascii_whitespace())?;
    let after = after.trim_start();
    let bytes = after.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        let c = bytes[i];
        if c.is_ascii_alphanumeric() || c == b'_' || c == b'$' {
            i += 1;
        } else {
            break;
        }
    }
    if i == 0 {
        return None;
    }
    let ident = &after[..i];
    // Anything after the identifier other than whitespace (`.`, `(`, `<`)
    // means it's a member access / call / generic — not a bare sibling.
    let tail = after[i..].trim_start();
    if !tail.is_empty() {
        return None;
    }
    Some(ident.to_string())
}

/// Issue #2310 — true if `class_body` contains any of the given names as a
/// bare identifier (word-boundary match). Used to gate the hoist in
/// `extract_top_level_class_decls`. We don't try to be precise about
/// shadowing inside the class body — class-private fields and method
/// parameters shadow only inside their scope, and the cost of a false
/// "don't hoist" is a slower lookup (closure-captured) rather than a
/// miscompile, so the conservative answer is acceptable here.
fn class_body_references_any(class_body: &str, names: &[String]) -> bool {
    if names.is_empty() {
        return false;
    }
    let bytes = class_body.as_bytes();
    for name in names {
        let nbytes = name.as_bytes();
        if nbytes.is_empty() {
            continue;
        }
        let mut i = 0usize;
        while i + nbytes.len() <= bytes.len() {
            if &bytes[i..i + nbytes.len()] == nbytes {
                let before_ok = i == 0
                    || !(bytes[i - 1].is_ascii_alphanumeric()
                        || bytes[i - 1] == b'_'
                        || bytes[i - 1] == b'$'
                        || bytes[i - 1] == b'.');
                let after = i + nbytes.len();
                let after_ok = after >= bytes.len()
                    || !(bytes[after].is_ascii_alphanumeric()
                        || bytes[after] == b'_'
                        || bytes[after] == b'$');
                if before_ok && after_ok {
                    return true;
                }
            }
            i += 1;
        }
    }
    false
}
