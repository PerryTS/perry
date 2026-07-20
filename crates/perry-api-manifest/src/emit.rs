//! Markdown + TypeScript-declaration serializers for `API_MANIFEST`.
//!
//! Closes the docs / `.d.ts` half of #465. The compiler's
//! `--print-api-manifest=markdown` and `--print-api-manifest=dts` flags
//! delegate to these. Output is deterministic — modules sort
//! alphabetically, entries within a module sort by kind then name —
//! so regenerated docs produce stable diffs in CI.

use crate::{
    entry_is_public_named_export, is_node_core_private_named_export, ApiEntry, ApiKind, ApiSource,
    ParamSpec, TypeSpec, API_MANIFEST,
};
use std::collections::BTreeMap;
use std::fmt::Write;

/// Render the manifest as a single combined Markdown reference page.
/// Compiler version is interpolated into the header so consumers can
/// tell at a glance which Perry release the doc was generated from.
pub fn emit_markdown(_perry_version: &str) -> String {
    let mut out = String::new();
    let by_module = group_by_module(entry_visible_in_markdown);

    let _ = writeln!(out, "# Supported API Reference");
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "This page is auto-generated from Perry's compile-time API manifest \
         (`perry-api-manifest::API_MANIFEST`). It is the source of truth for \
         what `perry compile` accepts; references to symbols not listed here \
         produce `R005 UnimplementedApi` (issue #463). Stubs (#464) are \
         flagged ⚠ — they link cleanly but no-op at runtime on the chosen target."
    );
    let _ = writeln!(out);
    // Note: deliberately NOT embedding the Perry version here — pre-fix
    // the line `**Generated for Perry v{version}.**` made every patch-version
    // bump trigger the `api-docs-drift` CI gate even when the manifest
    // itself was unchanged. The artifact is now version-independent;
    // version info lives in Cargo.toml / CLAUDE.md.
    let _ = writeln!(
        out,
        "Total: {} entries across {} modules.",
        by_module.values().map(|v| v.len()).sum::<usize>(),
        by_module.len()
    );
    let _ = writeln!(out);
    let _ = writeln!(out, "## Modules");
    let _ = writeln!(out);
    for module in by_module.keys() {
        let _ = writeln!(out, "- [`{}`](#{})", module, anchor(module));
    }
    let _ = writeln!(out);
    let _ = writeln!(out, "---");
    let _ = writeln!(out);

    for (module, entries) in &by_module {
        let _ = writeln!(out, "## `{}`", module);
        let _ = writeln!(out);

        let methods: Vec<&ApiEntry> = entries
            .iter()
            .copied()
            .filter(|e| match e.kind {
                ApiKind::Method {
                    has_receiver: true, ..
                } => !is_node_core_private_named_export(e.module, e.name),
                ApiKind::Method {
                    class_filter: Some(_),
                    ..
                } => !is_node_core_private_named_export(e.module, e.name),
                ApiKind::Method { .. } => entry_is_public_named_export(e),
                _ => false,
            })
            .collect();
        let properties: Vec<&ApiEntry> = entries
            .iter()
            .copied()
            .filter(|e| matches!(e.kind, ApiKind::Property) && entry_is_public_named_export(e))
            .collect();
        let classes: Vec<&ApiEntry> = entries
            .iter()
            .copied()
            .filter(|e| matches!(e.kind, ApiKind::Class) && entry_is_public_named_export(e))
            .collect();

        if !classes.is_empty() {
            let _ = writeln!(out, "### Classes");
            let _ = writeln!(out);
            for e in &classes {
                let _ = writeln!(out, "- `{}`{}", e.name, source_marker(e));
            }
            let _ = writeln!(out);
        }

        if !methods.is_empty() {
            let _ = writeln!(out, "### Methods");
            let _ = writeln!(out);
            for e in &methods {
                if let ApiKind::Method {
                    has_receiver,
                    class_filter,
                } = e.kind
                {
                    let receiver = if has_receiver { "instance" } else { "module" };
                    let cls = class_filter
                        .map(|c| format!(" *(class: `{}`)*", c))
                        .unwrap_or_default();
                    let _ = writeln!(
                        out,
                        "- `{}` — {}{}{}",
                        e.name,
                        receiver,
                        cls,
                        source_marker(e),
                    );
                }
            }
            let _ = writeln!(out);
        }

        if !properties.is_empty() {
            let _ = writeln!(out, "### Properties");
            let _ = writeln!(out);
            for e in &properties {
                let _ = writeln!(out, "- `{}`{}", e.name, source_marker(e));
            }
            let _ = writeln!(out);
        }
    }

    trim_trailing_blank_line(out)
}

/// Render the manifest as a TypeScript declaration file (`.d.ts`).
/// Editors that load this get squiggles on unimplemented references
/// before `perry compile` runs.
///
/// As of #512 module-level functions (no-receiver, no class_filter)
/// render with their declared `params` / `returns` from the manifest.
/// Entries without signature data fall back to the loose
/// `(...args: any[]): any` shape so behavior never regresses for un-typed
/// rows. Instance methods and class-filtered rows still hang off
/// `[key: string]: any;` on their class — narrowing those needs a
/// follow-up that threads receiver-type info through HIR.
pub fn emit_dts(_perry_version: &str) -> String {
    let mut out = String::new();
    let by_module = group_by_module(entry_visible_in_dts);

    let _ = writeln!(
        out,
        "// Auto-generated from Perry's API manifest (#465). Do not edit by hand."
    );
    let _ = writeln!(out, "// Source: perry-api-manifest::API_MANIFEST");
    // Note: deliberately NOT embedding the Perry version here — pre-fix
    // every patch-version bump triggered the `api-docs-drift` CI gate even
    // when the manifest itself was unchanged. The artifact is now version-
    // independent; version info lives in Cargo.toml / CLAUDE.md.
    let _ = writeln!(
        out,
        "// Coverage: {} entries across {} modules",
        by_module.values().map(|v| v.len()).sum::<usize>(),
        by_module.len()
    );
    let _ = writeln!(out);
    emit_native_memory_globals(&mut out);
    let _ = writeln!(out);

    for (module, entries) in &by_module {
        let module_decl = module_declaration_name(module);
        let _ = writeln!(out, "declare module \"{}\" {{", module_decl);

        // Classes first — methods may reference them via class_filter.
        for e in entries
            .iter()
            .filter(|e| matches!(e.kind, ApiKind::Class) && entry_is_public_named_export(e))
        {
            let _ = writeln!(out, "  /** {}{} */", source_dts_tag(e), stub_dts_suffix(e));
            let _ = writeln!(
                out,
                "  export class {} {{ [key: string]: any; }}",
                ts_ident(e.name)
            );
        }

        // Properties.
        for e in entries
            .iter()
            .filter(|e| matches!(e.kind, ApiKind::Property) && entry_is_public_named_export(e))
        {
            let _ = writeln!(out, "  /** {}{} */", source_dts_tag(e), stub_dts_suffix(e));
            if e.name == "default" {
                let _ = writeln!(out, "  const _default: any;");
                let _ = writeln!(out, "  export default _default;");
            } else {
                let _ = writeln!(out, "  export const {}: any;", ts_ident(e.name));
            }
        }

        // Module-level functions (has_receiver: false, no class_filter).
        // Instance methods (has_receiver: true) and class-filtered ones
        // hang off classes that aren't reflected in the manifest's
        // method entries — `[key: string]: any;` on the class above
        // makes their access compile, just without IDE squiggle help.
        // Followup under #466 will tighten this when signature data lands.
        let mut emitted_fn_names: std::collections::HashSet<&str> =
            std::collections::HashSet::new();
        for e in entries.iter().filter(|e| {
            matches!(
                e.kind,
                ApiKind::Method {
                    has_receiver: false,
                    class_filter: None,
                }
            ) && entry_is_public_named_export(e)
        }) {
            // Same name can appear with multiple class_filter rows in
            // the dispatch table; the manifest collapses them but a
            // duplicate-emit guard keeps the output well-formed.
            if !emitted_fn_names.insert(e.name) {
                continue;
            }
            let _ = writeln!(out, "  /** {}{} */", source_dts_tag(e), stub_dts_suffix(e));
            // `default` is the npm convention for "the module is
            // callable" (e.g. `import sharp from 'sharp'`). TypeScript
            // expresses that as `export default function (...)`, with
            // no name on the function declaration.
            let signature = render_signature(e);
            if e.name == "default" {
                let _ = writeln!(out, "  export default function {};", signature);
            } else if is_ts_reserved_word(e.name) {
                // Reserved words (e.g. `axios.delete`) can't appear as
                // a function declaration's name — `tsc` rejects
                // `export function delete(...)` with TS1359 (#526).
                // Declare under an underscored alias and re-export with
                // the original name; the `as <reserved>` rename slot
                // accepts arbitrary identifiers.
                let alias = format!("_{}", e.name);
                let _ = writeln!(out, "  function {}{};", alias, signature);
                let _ = writeln!(out, "  export {{ {} as {} }};", alias, e.name);
            } else {
                let _ = writeln!(out, "  export function {}{};", ts_ident(e.name), signature);
            }
        }

        let _ = writeln!(out, "}}");
        let _ = writeln!(out);
    }

    trim_trailing_blank_line(out)
}

// -----------------------------------------------------------------------------
// helpers
// -----------------------------------------------------------------------------

fn trim_trailing_blank_line(mut out: String) -> String {
    while out.ends_with("\n\n") {
        out.pop();
    }
    out
}

fn group_by_module(
    include: fn(&ApiEntry) -> bool,
) -> BTreeMap<&'static str, Vec<&'static ApiEntry>> {
    let mut by_module: BTreeMap<&'static str, Vec<&'static ApiEntry>> = BTreeMap::new();
    for entry in API_MANIFEST {
        if !include(entry) {
            continue;
        }
        by_module.entry(entry.module).or_default().push(entry);
    }
    for entries in by_module.values_mut() {
        entries.sort_by_key(|e| (kind_order(&e.kind), e.name));
    }
    by_module
}

fn entry_visible_in_markdown(entry: &ApiEntry) -> bool {
    match entry.kind {
        ApiKind::Method {
            has_receiver: true, ..
        }
        | ApiKind::Method {
            class_filter: Some(_),
            ..
        } => true,
        ApiKind::Method { .. } | ApiKind::Property | ApiKind::Class => {
            entry_is_public_named_export(entry)
        }
    }
}

fn entry_visible_in_dts(entry: &ApiEntry) -> bool {
    match entry.kind {
        ApiKind::Method {
            has_receiver: false,
            class_filter: None,
        }
        | ApiKind::Property
        | ApiKind::Class => entry_is_public_named_export(entry),
        ApiKind::Method { .. } => false,
    }
}

fn emit_native_memory_globals(out: &mut String) {
    let _ = writeln!(
        out,
        "type PerryU32 = number & {{ readonly __perryU32?: never }};"
    );
    let _ = writeln!(
        out,
        "type PerryU64 = number & {{ readonly __perryU64?: never }};"
    );
    let _ = writeln!(
        out,
        "type PerryUSize = number & {{ readonly __perryUSize?: never }};"
    );
    let _ = writeln!(
        out,
        "type PerryI32 = number & {{ readonly __perryI32?: never }};"
    );
    let _ = writeln!(
        out,
        "type PerryI64 = number & {{ readonly __perryI64?: never }};"
    );
    let _ = writeln!(
        out,
        "type PerryF32 = number & {{ readonly __perryF32?: never }};"
    );
    let _ = writeln!(
        out,
        "type PerryF64 = number & {{ readonly __perryF64?: never }};"
    );
    let _ = writeln!(
        out,
        "type PerryBufferLen = number & {{ readonly __perryBufferLen?: never }};"
    );
    let _ = writeln!(
        out,
        "type PerryHandleId = number & {{ readonly __perryHandleId?: never }};"
    );
    let _ = writeln!(
        out,
        "type PerryPod<T> = T & {{ readonly __perryPod?: never }};"
    );
    let _ = writeln!(
        out,
        "type NativeMemoryTypedView = Int8Array | Uint8Array | Uint8ClampedArray | Int16Array | Uint16Array | Int32Array | Uint32Array | Float32Array | Float64Array;"
    );
    let _ = writeln!(
        out,
        "declare function sizeof<T extends PerryPod<any>>(): number;"
    );
    let _ = writeln!(
        out,
        "declare function alignof<T extends PerryPod<any>>(): number;"
    );
    let _ = writeln!(
        out,
        "declare function offsetof<T extends PerryPod<any>>(field: string): number;"
    );
    let _ = writeln!(out, "interface PerryPodView<T> {{");
    let _ = writeln!(out, "  readonly length: number;");
    let _ = writeln!(out, "  readonly [index: number]: T;");
    let _ = writeln!(out, "  readonly __perryPodView?: never;");
    let _ = writeln!(out, "}}");
    let _ = writeln!(out, "interface NativeArena {{");
    for (ctor, ret) in native_arena_view_overloads() {
        let _ = writeln!(
            out,
            "  view(kind: typeof {}, byteOffset: number, length: number): {};",
            ctor, ret
        );
    }
    for (name, ret) in native_arena_view_overloads() {
        let _ = writeln!(
            out,
            "  view(kind: \"{}\", byteOffset: number, length: number): {};",
            name, ret
        );
    }
    let _ = writeln!(
        out,
        "  podView<T extends PerryPod<any>>(byteOffset: number, count: number): PerryPodView<T>;"
    );
    let _ = writeln!(out, "  dispose(): void;");
    let _ = writeln!(out, "}}");
    let _ = writeln!(out, "interface NativeArenaConstructor {{");
    let _ = writeln!(out, "  alloc(byteLength: number): NativeArena;");
    let _ = writeln!(out, "}}");
    let _ = writeln!(out, "declare const NativeArena: NativeArenaConstructor;");
    let _ = writeln!(out, "interface NativeMemoryConstructor {{");
    let _ = writeln!(out, "  fillU32(view: Uint32Array, value: number): void;");
    let _ = writeln!(
        out,
        "  copy(dst: NativeMemoryTypedView, src: NativeMemoryTypedView): void;"
    );
    let _ = writeln!(out, "}}");
    let _ = writeln!(out, "declare const NativeMemory: NativeMemoryConstructor;");
}

fn native_arena_view_overloads() -> &'static [(&'static str, &'static str)] {
    &[
        ("Int8Array", "Int8Array"),
        ("Uint8Array", "Uint8Array"),
        ("Uint8ClampedArray", "Uint8ClampedArray"),
        ("Int16Array", "Int16Array"),
        ("Uint16Array", "Uint16Array"),
        ("Int32Array", "Int32Array"),
        ("Uint32Array", "Uint32Array"),
        ("Float32Array", "Float32Array"),
        ("Float64Array", "Float64Array"),
    ]
}

fn kind_order(kind: &ApiKind) -> u8 {
    match kind {
        ApiKind::Class => 0,
        ApiKind::Property => 1,
        ApiKind::Method { .. } => 2,
    }
}

fn source_marker(entry: &ApiEntry) -> String {
    let mut tag = match entry.source {
        ApiSource::Stdlib => String::new(),
        ApiSource::WellKnown => " *(well-known)*".to_string(),
        ApiSource::External => " *(external)*".to_string(),
        ApiSource::Intrinsic => " *(intrinsic)*".to_string(),
    };
    if entry.stub {
        match entry.stub_note {
            Some(note) => {
                let _ = write!(tag, " ⚠ **stub** — {}", note);
            }
            None => tag.push_str(" ⚠ **stub** (no-op at runtime)"),
        }
    }
    tag
}

/// `.d.ts` doc-comment suffix for a stub entry: an `@perryStub` JSDoc
/// tag (editors surface it on hover, mirroring `@deprecated`) plus the
/// per-entry note when present. Empty string for real APIs. (#4918)
fn stub_dts_suffix(entry: &ApiEntry) -> String {
    if !entry.stub {
        return String::new();
    }
    match entry.stub_note {
        Some(note) => format!(" @perryStub {}", note),
        None => " @perryStub stub — no-op at runtime".to_string(),
    }
}

fn source_dts_tag(entry: &ApiEntry) -> &'static str {
    match entry.source {
        ApiSource::Stdlib => "stdlib",
        ApiSource::WellKnown => "well-known",
        ApiSource::External => "external",
        ApiSource::Intrinsic => "intrinsic",
    }
}

/// Markdown anchor for a heading, matching mdbook's `normalize_id`
/// (github-slugifier): keep alphanumerics + `_`/`-`, lowercase, turn
/// whitespace into `-`, and DROP every other punctuation character.
///
/// The distinction matters for module names with punctuation: mdbook
/// slugs `bun:ffi` → `bunffi` and `mysql2/promise` → `mysql2promise`
/// (the `:` / `/` are removed, NOT turned into `-`). The previous
/// "replace every non-alphanumeric with `-`" produced `bun-ffi` /
/// `mysql2-promise`, so the in-page TOC links jumped nowhere. (#6580)
fn anchor(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-' || c.is_whitespace())
        .map(|c| {
            if c.is_whitespace() {
                '-'
            } else {
                c.to_ascii_lowercase()
            }
        })
        .collect()
}

/// `mysql2/promise` becomes `mysql2/promise` — TS allows slash in
/// module specifiers. `perry/ui` → `perry/ui`. No transformation
/// needed today; kept as a hook for #466 Phase 2 if external manifests
/// ever need namespacing.
fn module_declaration_name(s: &str) -> &str {
    s
}

/// Render a method's parenthesized signature `(p0: string, p1: number): boolean`.
/// Falls back to `(...args: any[]): any` when the entry has no params and
/// returns `Any` so un-typed rows don't regress today's loose shape (#512).
fn render_signature(entry: &ApiEntry) -> String {
    let has_params = !entry.params.is_empty();
    let has_return = entry.returns != TypeSpec::Any;
    if !has_params && !has_return {
        // Fall through to today's loose default — no usable signature data.
        return "(...args: any[]): any".to_string();
    }

    let mut out = String::from("(");
    let mut first = true;
    for p in entry.params {
        if !first {
            out.push_str(", ");
        }
        first = false;
        match p {
            ParamSpec::Named { name, ty, optional } => {
                out.push_str(name);
                if *optional {
                    out.push('?');
                }
                out.push_str(": ");
                out.push_str(render_type(ty));
            }
            ParamSpec::Rest { name, ty } => {
                out.push_str("...");
                out.push_str(name);
                out.push_str(": ");
                out.push_str(render_type(ty));
                out.push_str("[]");
            }
        }
    }
    out.push_str("): ");
    out.push_str(render_type(&entry.returns));
    out
}

/// Render a [`TypeSpec`] as the TypeScript type the .d.ts emitter
/// should print. Mirrors the param/return-type vocabulary in
/// `docs/src/native-libraries/manifest-v1.md`.
fn render_type(ty: &TypeSpec) -> &'static str {
    match ty {
        TypeSpec::String => "string",
        TypeSpec::Number => "number",
        TypeSpec::Bool => "boolean",
        TypeSpec::BigInt => "bigint",
        TypeSpec::Buffer => "Buffer",
        TypeSpec::Handle => "any",
        TypeSpec::Void => "void",
        TypeSpec::Any => "any",
        TypeSpec::Promise => "Promise<any>",
    }
}

/// TS keywords that can't appear as a function declaration's name.
/// Property accesses (`obj.delete()`) accept these, but a top-level
/// `export function delete(...)` is rejected by `tsc` with TS1359 — so
/// the .d.ts emitter routes around it via `function _delete(...); export
/// { _delete as delete };` (the `as <name>` rename slot accepts
/// arbitrary identifiers including reserved words). #526.
fn is_ts_reserved_word(s: &str) -> bool {
    matches!(
        s,
        // Strict-mode reserved words.
        "break"
            | "case"
            | "catch"
            | "class"
            | "const"
            | "continue"
            | "debugger"
            | "default"
            | "delete"
            | "do"
            | "else"
            | "enum"
            | "export"
            | "extends"
            | "false"
            | "finally"
            | "for"
            | "function"
            | "if"
            | "import"
            | "in"
            | "instanceof"
            | "new"
            | "null"
            | "return"
            | "super"
            | "switch"
            | "this"
            | "throw"
            | "true"
            | "try"
            | "typeof"
            | "var"
            | "void"
            | "while"
            | "with"
            | "yield"
            // Strict-mode-only contextual keywords. `tsc` rejects most
            // of these as function names too.
            | "implements"
            | "interface"
            | "let"
            | "package"
            | "private"
            | "protected"
            | "public"
            | "static"
            | "await"
    )
}

/// TypeScript identifiers can't start with a digit and forbid most
/// punctuation. Manifest names are already valid identifiers in
/// practice; this is just defensive in case a future entry adds one.
fn ts_ident(s: &str) -> String {
    let mut out: String = s
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '$' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if out
        .chars()
        .next()
        .map(|c| c.is_ascii_digit())
        .unwrap_or(false)
    {
        out.insert(0, '_');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_contains_every_visible_module() {
        let md = emit_markdown("test");
        let modules: std::collections::HashSet<&'static str> = API_MANIFEST
            .iter()
            .filter(|entry| entry_visible_in_markdown(entry))
            .map(|e| e.module)
            .collect();
        for m in &modules {
            // Modules render as `## `<name>``.
            assert!(
                md.contains(&format!("## `{}`", m)),
                "module heading missing: {}",
                m
            );
        }
    }

    #[test]
    fn dts_declares_every_visible_module() {
        let dts = emit_dts("test");
        let modules: std::collections::HashSet<&'static str> = API_MANIFEST
            .iter()
            .filter(|entry| entry_visible_in_dts(entry))
            .map(|e| e.module)
            .collect();
        for m in &modules {
            assert!(
                dts.contains(&format!("declare module \"{}\"", m)),
                "module declaration missing: {}",
                m
            );
        }
    }

    #[test]
    fn emitters_omit_internal_punycode_ucs2_module() {
        let md = emit_markdown("test");
        let dts = emit_dts("test");
        assert!(!md.contains("## `punycode.ucs2`"));
        assert!(!dts.contains("declare module \"punycode.ucs2\""));
    }

    #[test]
    fn emitters_end_with_one_newline() {
        for output in [emit_markdown("test"), emit_dts("test")] {
            assert!(
                output.ends_with('\n'),
                "generated docs should end with newline"
            );
            assert!(
                !output.ends_with("\n\n"),
                "generated docs should not end with a trailing blank line"
            );
        }
    }

    #[test]
    fn dts_known_method_appears() {
        let dts = emit_dts("test");
        // crypto.randomUUID is a stable, no-receiver method — should
        // surface as `export function randomUUID(...)` in `declare
        // module "crypto"`.
        let crypto_block_start = dts.find("declare module \"crypto\"").expect("crypto block");
        let after = &dts[crypto_block_start..];
        let crypto_block_end = after.find("\n}\n").expect("block end");
        let crypto_block = &after[..crypto_block_end];
        assert!(
            crypto_block.contains("export function randomUUID"),
            "crypto.randomUUID missing from .d.ts"
        );
    }

    #[test]
    fn worker_threads_internal_receiver_methods_stay_out_of_docs() {
        let md = emit_markdown("test");
        let dts = emit_dts("test");
        assert!(
            !md.contains("`postMessage` — instance"),
            "worker_threads.postMessage should stay receiver-dispatch-only"
        );
        assert!(
            !dts.contains("export function postMessage("),
            "worker_threads.postMessage should not be emitted as a module function"
        );
    }

    /// #512 acceptance contract: `bcrypt.hash` must declare its first
    /// param as `string` so a `bcrypt.hash(123, "salt")` call site is
    /// rejected by `tsc`. Untyped (`...args: any[]`) signatures pass
    /// such calls silently — that's exactly what #512 carves out.
    #[test]
    fn dts_bcrypt_hash_has_real_signature() {
        let dts = emit_dts("test");
        let block_start = dts.find("declare module \"bcrypt\"").expect("bcrypt block");
        let after = &dts[block_start..];
        let block_end = after.find("\n}\n").expect("block end");
        let block = &after[..block_end];
        assert!(
            block.contains("export function hash(password: string"),
            "bcrypt.hash should declare password: string\nblock: {}",
            block
        );
        // Defense-in-depth: the loose fallback shape must NOT appear
        // for hash — that would mean the backfill regressed.
        assert!(
            !block.contains("export function hash(...args: any[]): any"),
            "bcrypt.hash regressed to loose any signature\nblock: {}",
            block
        );
    }

    /// #526 acceptance: a method named after a TS reserved word must
    /// not surface as `export function <reserved>(...)` — `tsc` errors
    /// out with TS1359. The emitter routes through the
    /// `function _delete; export { _delete as delete }` alias pattern
    /// so a fresh `perry init` project's `tsc -p .` succeeds.
    #[test]
    fn dts_axios_delete_does_not_use_reserved_word_as_fn_name() {
        let dts = emit_dts("test");
        let block_start = dts.find("declare module \"axios\"").expect("axios block");
        let after = &dts[block_start..];
        let block_end = after.find("\n}\n").expect("block end");
        let block = &after[..block_end];
        assert!(
            !block.contains("export function delete("),
            "axios.delete must not be emitted as `export function delete(` (TS1359)\nblock: {}",
            block
        );
        assert!(
            block.contains("function _delete(") && block.contains("_delete as delete"),
            "axios.delete should use the `function _delete; export {{ _delete as delete }}` \
             alias pattern\nblock: {}",
            block
        );
    }

    /// Defense-in-depth for #526: every reserved word the emitter
    /// recognizes should round-trip through the alias pattern, so
    /// future manifest additions (e.g. `axios.try`, `axios.new`) don't
    /// silently re-break the .d.ts. We synthesize the test by walking
    /// the live manifest — any module-level function whose name is a
    /// reserved word must not appear under `export function <name>(`.
    #[test]
    fn dts_no_reserved_word_function_declarations() {
        let dts = emit_dts("test");
        for entry in API_MANIFEST {
            if !matches!(
                entry.kind,
                ApiKind::Method {
                    has_receiver: false,
                    class_filter: None,
                }
            ) {
                continue;
            }
            if !is_ts_reserved_word(entry.name) {
                continue;
            }
            let bad = format!("export function {}(", entry.name);
            assert!(
                !dts.contains(&bad),
                "{} module: reserved-word method `{}` leaked as `export function {}(`",
                entry.module,
                entry.name,
                entry.name
            );
        }
    }

    /// uuid.v4() is no-args and returns a string — verify the renderer
    /// emits an empty arg list instead of `(...args: any[])`.
    #[test]
    fn dts_uuid_v4_has_no_args() {
        let dts = emit_dts("test");
        let block_start = dts.find("declare module \"uuid\"").expect("uuid block");
        let after = &dts[block_start..];
        let block_end = after.find("\n}\n").expect("block end");
        let block = &after[..block_end];
        assert!(
            block.contains("export function v4(): string"),
            "uuid.v4 should be (): string\nblock: {}",
            block
        );
    }
}
