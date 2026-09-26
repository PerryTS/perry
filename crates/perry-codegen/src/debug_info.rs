//! Source-level DWARF line tables under `--debug-symbols` (discussion #9856).
//!
//! Before this, `PERRY_DEBUG_SYMBOLS` handed clang `-g`, but Perry's IR carried
//! no `DICompileUnit`/`DISubprogram`/`!dbg` metadata, so the object had no
//! `.debug_line` at all and no debugger (lldb in Xcode, CodeLLDB in VS Code,
//! gdb) could bind a `file.ts:line` breakpoint.
//!
//! Two halves, deliberately decoupled:
//!
//! 1. **Codegen drops markers.** [`mark_expr_location`] runs at the top of
//!    `lower_expr` and, for every expression that carries a source byte offset
//!    (calls, `new`, member reads — the same offsets #5247 already resolves for
//!    error `.stack`s), emits a comment line `; perrydbg <line> <file>` into
//!    the current block. A comment is inert to every textual rewrite that runs
//!    between lowering and object emission (precise-root lowering, landing-pad
//!    retyping, invoke-EH phi rewriting), so none of them needs to learn about
//!    debug metadata.
//! 2. **The linker turns markers into metadata.** [`attach_line_tables`] runs
//!    on the final module text just before it is compiled: every `define` that
//!    contains a marker gets a `DISubprogram`, every instruction in it a
//!    `!dbg !DILocation(...)` for the most recent marker above it, and the
//!    module a `DICompileUnit` plus the `Debug Info Version` flag.
//!
//! Scope of this first slice: **lines and functions only**. Lines are known
//! where an expression carries an offset, so a statement that is pure
//! arithmetic on locals inherits the previous marked line; debuggers slide
//! such a breakpoint to the next line that has code. No variable/type info
//! (the compile unit is `FullDebug` only so that subprograms get DIEs and
//! frames are named).
//!
//! Every `define` in a module that has markers gets a `DISubprogram` and every
//! instruction in it a location (line 0 before the first marker of a function
//! that has none). That is what the LLVM verifier requires of inlinable calls
//! inside a function with debug info, and what keeps inlined bodies' lines.

use crate::expr::FnCtx;
use perry_hir::Expr;

/// Prefix of the marker comment. No `@`, `call` or `invoke` in it, so the
/// textual scanners that look for call sites never mistake one for a call.
pub(crate) const MARKER_PREFIX: &str = "; perrydbg ";

/// First metadata id used for debug info. Perry's own metadata is numbered
/// from `!0` upward and stays small; LLVM accepts sparse ids, so starting far
/// above them avoids collisions without renumbering anything.
const FIRST_DEBUG_MD_ID: u64 = 4_000_000_000;

/// Emit a line marker for `expr` if it carries a source offset that resolves
/// to a `file:line` in the current module. No-op unless `--debug-symbols`
/// installed a source-location context.
pub(crate) fn mark_expr_location(ctx: &mut FnCtx<'_>, expr: &Expr) {
    if !ctx.strings.debug_locations_enabled() {
        return;
    }
    let byte_offset = match expr {
        Expr::Call { byte_offset, .. }
        | Expr::PropertyGet { byte_offset, .. }
        | Expr::New { byte_offset, .. }
        | Expr::NewDynamic { byte_offset, .. }
        | Expr::NewDynamicSpread { byte_offset, .. } => *byte_offset,
        _ => return,
    };
    let Some((file, line)) = ctx.strings.call_location_for(byte_offset) else {
        return;
    };
    if file.contains('\n') {
        return;
    }
    let marker = format!("{MARKER_PREFIX}{line} {file}");
    ctx.block().emit_raw(marker);
}

fn parse_marker(line: &str) -> Option<(u32, &str)> {
    let rest = line.trim_start().strip_prefix(MARKER_PREFIX)?;
    let (num, file) = rest.split_once(' ')?;
    Some((num.parse().ok()?, file))
}

/// Quote a string for an LLVM metadata string literal.
fn llvm_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for b in s.bytes() {
        match b {
            b'"' | b'\\' => out.push_str(&format!("\\{b:02X}")),
            0x20..=0x7e => out.push(b as char),
            _ => out.push_str(&format!("\\{b:02X}")),
        }
    }
    out.push('"');
    out
}

/// Byte index of a trailing `;` comment on an instruction line, ignoring
/// semicolons inside quoted strings (`c"..."`, `!"..."`, quoted symbol names).
fn comment_start(line: &str) -> Option<usize> {
    let mut in_quote = false;
    for (i, b) in line.bytes().enumerate() {
        match b {
            b'"' => in_quote = !in_quote,
            b';' if !in_quote => return Some(i),
            _ => {}
        }
    }
    None
}

/// The first `@symbol` on a line (quotes stripped): the defined name on a
/// `define` header, the direct callee on a call. `None` for indirect calls.
fn symbol_after_at(line: &str) -> Option<&str> {
    let rest = line.split_once('@')?.1;
    let name = match rest.strip_prefix('"') {
        Some(quoted) => quoted.split_once('"')?.0,
        None => rest
            .split(|c: char| c == '(' || c.is_whitespace() || c == ',')
            .next()?,
    };
    (!name.is_empty()).then_some(name)
}

/// Whether an instruction line is a `call`/`invoke` (any tail-call flavour,
/// with or without a result).
fn is_call(inst: &str) -> bool {
    let op = match inst.split_once(" = ") {
        Some((lhs, rhs)) if !lhs.contains(' ') => rhs,
        _ => inst,
    };
    let op = op
        .strip_prefix("tail ")
        .or_else(|| op.strip_prefix("musttail "))
        .or_else(|| op.strip_prefix("notail "))
        .unwrap_or(op);
    op.starts_with("call ") || op.starts_with("invoke ")
}

/// Split an absolute-or-relative source path into LLVM's `(filename,
/// directory)`. Relative paths are resolved against the compiler's working
/// directory, which is where the driver read them from.
fn split_path(file: &str) -> (String, String) {
    let path = std::path::Path::new(file);
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::fs::canonicalize(path).unwrap_or_else(|_| {
            std::env::current_dir()
                .map(|d| d.join(path))
                .unwrap_or_else(|_| path.to_path_buf())
        })
    };
    let name = abs
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| file.to_string());
    let dir = abs
        .parent()
        .map(|d| d.to_string_lossy().into_owned())
        .unwrap_or_default();
    (name, dir)
}

/// Rewrite module text carrying `; perrydbg` markers into text carrying real
/// DWARF line-table metadata. Returns `None` when the text has no markers, so
/// callers can skip the copy (and so a module that never saw `--debug-symbols`
/// is provably untouched).
pub fn attach_line_tables(ll: &str) -> Option<String> {
    if !ll.contains(MARKER_PREFIX) {
        return None;
    }

    let mut next_id = FIRST_DEBUG_MD_ID;
    let mut fresh = || {
        let id = next_id;
        next_id += 1;
        id
    };
    let cu_id = fresh();
    let subroutine_type_id = fresh();
    let mut file_ids: Vec<(String, u64)> = Vec::new();
    let mut metadata: Vec<String> = Vec::new();

    let mut out = String::with_capacity(ll.len() + ll.len() / 2);
    let lines: Vec<&str> = ll.lines().collect();
    let Some(module_file) = lines.iter().find_map(|l| parse_marker(l)).map(|(_, f)| f) else {
        return None;
    };
    // Each marked function's first line, for the delegate rule below.
    let mut first_lines: std::collections::HashMap<&str, (u32, &str)> =
        std::collections::HashMap::new();
    let mut current_fn: Option<&str> = None;
    for l in &lines {
        if l.starts_with("define ") {
            current_fn = symbol_after_at(l);
        } else if *l == "}" {
            current_fn = None;
        } else if let (Some(f), Some(m)) = (current_fn, parse_marker(l)) {
            first_lines.entry(f).or_insert(m);
        }
    }
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        if !line.starts_with("define ") {
            // Markers only ever sit inside bodies; drop any stray one anyway.
            if parse_marker(line).is_none() {
                out.push_str(line);
                out.push('\n');
            }
            i += 1;
            continue;
        }

        // Collect the body: everything up to the closing `}` line.
        let start = i;
        let mut end = i + 1;
        while end < lines.len() && lines[end] != "}" {
            end += 1;
        }
        let body = &lines[start + 1..end.min(lines.len())];

        // The function's file is the one its first marker names. Markers for
        // any other file (code inlined from another module at HIR level) are
        // ignored: a DILocation's line is relative to its scope's file.
        //
        // A function WITHOUT markers (a specialization's generic entry
        // wrapper, closure trampolines, init helpers) still gets a subprogram,
        // at line 0 ("no source line"): LLVM drops the locations of anything
        // it inlines into a caller that has no `DISubprogram`, so leaving the
        // wrappers bare would erase the lines of the very bodies they inline.
        //
        // Except when it directly calls a marked function of this module: a
        // specialization's entry wrapper (type dispatch, then a call to
        // `$spec`/`$generic`) or a closure trampoline is that function as far
        // as a user is concerned, so it takes the callee's line. Otherwise
        // the wrapper's line-0 dispatch code lands at the start of the very
        // block the inlined body is entered through, and a breakpoint on the
        // body's first line binds past the executed path.
        let first_marker = body.iter().find_map(|l| parse_marker(l)).or_else(|| {
            body.iter()
                .filter(|l| is_call(l.trim()))
                .filter_map(|l| symbol_after_at(l))
                .find_map(|callee| first_lines.get(callee).copied())
        });
        let marked = first_marker.is_some();
        let (first_line, file) = first_marker.unwrap_or((0, module_file));

        let file_id = match file_ids.iter().find(|(f, _)| f == file) {
            Some((_, id)) => *id,
            None => {
                let id = fresh();
                let (name, dir) = split_path(file);
                metadata.push(format!(
                    "!{id} = !DIFile(filename: {}, directory: {})",
                    llvm_quote(&name),
                    llvm_quote(&dir)
                ));
                file_ids.push((file.to_string(), id));
                id
            }
        };

        let sp_id = fresh();
        let header = line;
        let name = symbol_after_at(header).unwrap_or("");
        let local = header.starts_with("define internal ") || header.starts_with("define private ");
        metadata.push(format!(
            "!{sp_id} = distinct !DISubprogram(name: {q}, linkageName: {q}, scope: !{file_id}, \
             file: !{file_id}, line: {first_line}, type: !{subroutine_type_id}, \
             scopeLine: {first_line}, spFlags: {local}DISPFlagDefinition | DISPFlagOptimized, unit: !{cu_id})",
            q = llvm_quote(name),
            local = if local { "DISPFlagLocalToUnit | " } else { "" },
        ));

        // `define ... {` -> `define ... !dbg !N {`
        match header.strip_suffix('{') {
            Some(head) => {
                out.push_str(head.trim_end());
                out.push_str(&format!(" !dbg !{sp_id} {{\n"));
            }
            None => {
                out.push_str(header);
                out.push('\n');
            }
        }

        let mut current = first_line;
        let mut in_bracket = false;
        for l in body {
            if let Some((n, f)) = parse_marker(l) {
                if f == file {
                    current = n;
                }
                continue;
            }
            let trimmed = l.trim();
            let is_inst = l.starts_with(char::is_whitespace)
                && !trimmed.is_empty()
                && !trimmed.starts_with(';');
            if !is_inst {
                out.push_str(l);
                out.push('\n');
                continue;
            }
            // Multi-line `switch`/`indirectbr` operand lists: the attachment
            // goes after the closing `]`.
            if in_bracket {
                if trimmed == "]" {
                    in_bracket = false;
                } else {
                    out.push_str(l);
                    out.push('\n');
                    continue;
                }
            } else if trimmed.ends_with('[') {
                in_bracket = true;
                out.push_str(l);
                out.push('\n');
                continue;
            }
            // In a marker-less function only calls get a (line 0) location —
            // the verifier demands one on inlinable calls. Line-0 rows on
            // everything else get interleaved with the lines of bodies
            // inlined into it, and LLVM then withholds `is_stmt` from the
            // row where the real line resumes, so a breakpoint on that line
            // binds to an address the executed path never reaches.
            if !marked && !is_call(trimmed) {
                out.push_str(l);
                out.push('\n');
                continue;
            }
            let loc = format!(", !dbg !DILocation(line: {current}, scope: !{sp_id})");
            match comment_start(l) {
                Some(c) => {
                    out.push_str(l[..c].trim_end());
                    out.push_str(&loc);
                    out.push(' ');
                    out.push_str(&l[c..]);
                }
                None => {
                    out.push_str(l);
                    out.push_str(&loc);
                }
            }
            out.push('\n');
        }
        if end < lines.len() {
            out.push_str("}\n");
        }
        i = end + 1;
    }

    let flags_ver = fresh();
    let flags_dwarf = fresh();
    let primary_file = file_ids.first().map(|(_, id)| *id).unwrap_or(0);
    out.push('\n');
    out.push_str(&format!("!llvm.dbg.cu = !{{!{cu_id}}}\n"));
    out.push_str(&format!(
        "!llvm.module.flags = !{{!{flags_ver}, !{flags_dwarf}}}\n"
    ));
    out.push_str(&format!(
        "!{cu_id} = distinct !DICompileUnit(language: DW_LANG_C, file: !{primary_file}, \
         producer: \"perry\", isOptimized: true, runtimeVersion: 0, emissionKind: FullDebug)\n"
    ));
    out.push_str(&format!(
        "!{subroutine_type_id} = !DISubroutineType(types: !{{}})\n"
    ));
    out.push_str(&format!(
        "!{flags_ver} = !{{i32 2, !\"Debug Info Version\", i32 3}}\n"
    ));
    // MSVC targets want CodeView (it is what lands in the PDB `--debug-symbols`
    // asks the linker for); everything else gets DWARF 4, which Apple's
    // toolchain still defaults to and every debugger reads.
    let msvc = lines
        .iter()
        .take_while(|l| !l.starts_with("define "))
        .any(|l| l.starts_with("target triple") && l.contains("windows-msvc"));
    if msvc {
        out.push_str(&format!(
            "!{flags_dwarf} = !{{i32 2, !\"CodeView\", i32 1}}\n"
        ));
    } else {
        out.push_str(&format!(
            "!{flags_dwarf} = !{{i32 7, !\"Dwarf Version\", i32 4}}\n"
        ));
    }
    for m in metadata {
        out.push_str(&m);
        out.push('\n');
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const MODULE: &str = "source_filename = \"perry\"\n\
declare void @f()\n\
define internal double @perry_fn_main__g(double %a) #0 {\n\
entry:\n  %r1 = alloca double\n  ; perrydbg 3 /src/app.ts\n  call void @f() ; tail\n  br label %b\n\
b:\n  switch i32 0, label %c [\n    i32 1, label %c\n  ]\n\
c:\n  ; perrydbg 7 /src/app.ts\n  ret double %a\n}\n\
define double @wrap(double %a) {\n  %v = call double @perry_fn_main__g(double %a)\n  ret double %v\n}\n\
define void @nodebug() {\n  %x = fadd double 1.0, 2.0\n  %y = tail call double @g()\n  ret void\n}\n";

    #[test]
    fn untouched_without_markers() {
        assert!(attach_line_tables("define void @f() {\n  ret void\n}\n").is_none());
    }

    #[test]
    fn markers_become_locations() {
        let out = attach_line_tables(MODULE).unwrap();
        assert!(!out.contains(MARKER_PREFIX), "{out}");
        assert!(out.contains("define internal double @perry_fn_main__g(double %a) #0 !dbg !"));
        assert!(out.contains("  %r1 = alloca double, !dbg !DILocation(line: 3,"));
        assert!(out.contains("  call void @f(), !dbg !DILocation(line: 3,"));
        assert!(out.contains(" ; tail"));
        assert!(out.contains("    i32 1, label %c\n  ], !dbg !DILocation(line: 3,"));
        assert!(out.contains("  ret double %a, !dbg !DILocation(line: 7,"));
        // A delegate wrapper takes its callee's first line, on every instruction.
        assert!(out.contains("  ret double %v, !dbg !DILocation(line: 3,"));
        // A function without markers still gets a subprogram, at line 0, so
        // bodies LLVM inlines into it keep their lines.
        assert!(out.contains("define void @nodebug() !dbg !"));
        // ...but only its calls carry a location (see `is_call`).
        assert!(out.contains("  %y = tail call double @g(), !dbg !DILocation(line: 0,"));
        assert!(out.contains("  %x = fadd double 1.0, 2.0\n"));
        assert!(out.contains("  ret void\n"));
        assert!(out.contains("DISPFlagLocalToUnit"));
        assert!(out.contains("!DIFile(filename: \"app.ts\", directory: \"/src\")"));
        assert!(out.contains("!llvm.dbg.cu"));
        assert!(out.contains("!\"Dwarf Version\", i32 4"));
    }

    #[test]
    fn msvc_targets_get_codeview() {
        let module = format!("target triple = \"x86_64-pc-windows-msvc\"\n{MODULE}");
        let out = attach_line_tables(&module).unwrap();
        assert!(out.contains("!\"CodeView\", i32 1"));
        assert!(!out.contains("Dwarf Version"));
    }
}
