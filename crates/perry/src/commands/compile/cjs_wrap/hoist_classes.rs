//! Top-level `class` hoisting and `module.exports = class …` rewrite passes.

#[allow(unused_imports)]
use super::*;

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
    let mut hoisted_blocks: Vec<&str> = Vec::new();
    let mut hoisted_names: Vec<String> = Vec::new();
    let mut elided: Vec<(usize, usize)> = Vec::new();

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

        // Match optional leading whitespace.
        let mut p = line_start;
        while p < bytes.len() && (bytes[p] == b' ' || bytes[p] == b'\t') {
            p += 1;
        }

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
                        // Successful brace-balanced match. Record the block.
                        let block_text = std::str::from_utf8(&bytes[line_start..r]).unwrap_or("");
                        if !hoisted_names.contains(&class_name) {
                            hoisted_blocks.push(block_text);
                            hoisted_names.push(class_name);
                            elided.push((line_start, r));
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
