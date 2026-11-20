//! Node-API addons linked statically into a WASI program.
//!
//! WASI has no dynamic loader, so an approved addon's wasm32 archives
//! (`perry.nativeAddons` entry `{"name": <pkg>, "wasm": {<entry>: [<archive>…]}}`)
//! become part of the program link:
//!
//! 1. `wasm-ld -r` pre-links each addon's archives, rooted at its entry
//!    `napi_register_wasm_v1`, into one relocatable object.
//! 2. [`isolate`] rewrites that object's `linking` section: the entry (and the
//!    optional `node_api_module_get_api_version_v1`) get per-addon names, every
//!    other defined symbol becomes local, and COMDAT groups are dropped, so two
//!    addons that define the same symbols each keep their own. Undefined
//!    symbols (`napi_*`, libc, libc++) stay global and resolve in the final link
//!    against the runtime archive and the sysroot. `llvm-objcopy` cannot rename
//!    or localize wasm symbols.
//! 3. One generated object carries the compile-time manifest (the sidecar
//!    manifest's schema, `static-v1`) and each addon's entry pointers in
//!    manifest order, handed to the runtime by a constructor.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};

use super::super::super::wasi_flavour::Flavour;
use super::super::super::CompilationContext;

const ENTRY: &str = "napi_register_wasm_v1";
const VERSION: &str = "node_api_module_get_api_version_v1";

/// The objects and sysroot archives the program link adds for its addons.
pub(super) fn addon_link_inputs(
    flavour: Flavour,
    ctx: &CompilationContext,
    sdk: &Path,
    directory: &Path,
    verbose: u8,
) -> Result<Vec<PathBuf>> {
    let addons: Vec<_> = ctx
        .native_addons
        .values()
        .filter(|addon| !addon.wasm_archives.is_empty())
        .collect();
    if addons.is_empty() {
        return Ok(Vec::new());
    }
    let mut inputs = Vec::new();
    let mut manifest = Vec::new();
    let mut entries = Vec::new();
    for addon in &addons {
        let prefix = format!("__perry_addon_{}", short_hash(&addon.logical_id));
        let relocatable = directory.join(format!("{prefix}.r.o"));
        // `wasm-ld -r` refuses `-u`, so the entry is rooted by an object that
        // references it: archive members are pulled in from that reference,
        // exactly as the final link would pull them for a `-u`.
        let root = compile_c(
            flavour,
            sdk,
            directory,
            &format!("{prefix}_root"),
            &root_source(),
            verbose,
        )
        .with_context(|| format!("rooting Node-API addon `{}`", addon.logical_id))?;
        let mut command = Command::new(
            super::find_tool(sdk, "wasm-ld").unwrap_or_else(|| sdk.join("bin/wasm-ld")),
        );
        command
            .arg("-r")
            .arg("-o")
            .arg(&relocatable)
            .arg(&root)
            .args(&addon.wasm_archives);
        let status = super::super::super::tool_output::run_internal_tool(&mut command, verbose)?;
        std::fs::remove_file(&root)?;
        if !status.success() {
            bail!(
                "pre-linking the wasm archives of Node-API addon `{}` failed",
                addon.logical_id
            );
        }
        let bytes = std::fs::read(&relocatable)?;
        let register = format!("{prefix}_register");
        let version = format!("{prefix}_version");
        let (bytes, found) = isolate(&bytes, &register, &version)
            .with_context(|| format!("isolating Node-API addon `{}`", addon.logical_id))?;
        if !found.register {
            bail!(
                "the wasm archives of Node-API addon `{}` do not define `{ENTRY}`",
                addon.logical_id
            );
        }
        let object = directory.join(format!("{prefix}.o"));
        std::fs::write(&object, bytes)?;
        std::fs::remove_file(&relocatable)?;
        inputs.push(object);
        let entry = addon.entry_relative.to_string_lossy().replace('\\', "/");
        manifest.push(serde_json::json!({
            "logical_id": addon.logical_id,
            "require_aliases": [],
            "package": addon.package,
            "version": addon.version,
            "entry": entry,
            "files": [],
        }));
        entries.push((register, found.version.then_some(version)));
    }
    let manifest = serde_json::json!({
        "schema_version": super::super::super::native_addon_sidecar::NODE_API_POLICY_VERSION,
        "policy_version": super::super::super::native_addon_sidecar::NODE_API_POLICY_VERSION,
        "napi_version": super::super::super::native_addon_sidecar::NODE_API_VERSION,
        "shipping_model": "static-v1",
        "target": flavour.triple(),
        "addons": manifest,
    });
    let source = registration_source(&serde_json::to_string(&manifest)?, &entries);
    let object = compile_c(
        flavour,
        sdk,
        directory,
        "_perry_static_addons",
        &source,
        verbose,
    )
    .context("compiling the static Node-API addon manifest failed")?;
    inputs.push(object);
    // The addons' C++ runtime, linked once for all of them. Addons are built
    // without C++ exceptions (wasm EH belongs to Perry's own code), so the
    // noeh libc++; mmap emulation is what wasi-libc offers for allocators
    // such as abseil's.
    let sysroot = sdk.join("share/wasi-sysroot/lib").join(flavour.triple());
    for library in [
        "noeh/libc++.a",
        "noeh/libc++abi.a",
        "libwasi-emulated-mman.a",
    ] {
        let path = sysroot.join(library);
        if path.is_file() {
            inputs.push(path);
        }
    }
    Ok(inputs)
}

/// Compiles generated C `source` to `<directory>/<stem>.o` with the SDK clang.
fn compile_c(
    flavour: Flavour,
    sdk: &Path,
    directory: &Path,
    stem: &str,
    source: &str,
    verbose: u8,
) -> Result<PathBuf> {
    let c_path = directory.join(format!("{stem}.c"));
    let object = directory.join(format!("{stem}.o"));
    std::fs::write(&c_path, source)?;
    let mut command = Command::new(super::sdk_clang(sdk));
    command
        .arg(format!("--target={}", flavour.triple()))
        .arg(format!(
            "--sysroot={}",
            sdk.join("share/wasi-sysroot").display()
        ))
        .args(["-O2", "-c", "-o"])
        .arg(&object)
        .arg(&c_path);
    let status = super::super::super::tool_output::run_internal_tool(&mut command, verbose)?;
    std::fs::remove_file(&c_path)?;
    if !status.success() {
        bail!("clang failed on {}", c_path.display());
    }
    Ok(object)
}

/// The C source of an addon's root object: one reference to the entry, so the
/// relocatable pre-link pulls the archive members that define it. Its one
/// definition is made local by `isolate`, like every other addon symbol.
fn root_source() -> String {
    format!(
        "extern int {ENTRY}(void);\n\
         __attribute__((used)) static int (*const perry_addon_root)(void) = {ENTRY};\n"
    )
}

fn short_hash(logical_id: &str) -> String {
    let digest = Sha256::digest(logical_id.as_bytes());
    digest[..8].iter().map(|b| format!("{b:02x}")).collect()
}

/// The C source of the registration object: the manifest bytes and, in
/// manifest order, each addon's `{register, version}` pair.
fn registration_source(manifest: &str, entries: &[(String, Option<String>)]) -> String {
    let mut source = String::from("#include <stddef.h>\n");
    for (register, version) in entries {
        let _ = writeln!(source, "extern void *{register}(void *, void *);");
        if let Some(version) = version {
            let _ = writeln!(source, "extern int {version}(void);");
        }
    }
    source.push_str("static const char manifest[] = \"");
    for byte in manifest.bytes() {
        let _ = write!(source, "\\x{byte:02x}");
    }
    source.push_str("\";\n");
    source.push_str(
        "static const struct { void *(*reg)(void *, void *); int (*ver)(void); } entries[] = {\n",
    );
    for (register, version) in entries {
        let _ = writeln!(
            source,
            "  {{ {register}, {} }},",
            version.as_deref().unwrap_or("NULL")
        );
    }
    source.push_str("};\n");
    source.push_str(
        "extern void js_node_api_static_addons(const char *, size_t, const void *, size_t);\n\
         __attribute__((constructor)) static void perry_static_addons(void) {\n\
         \x20 js_node_api_static_addons(manifest, sizeof manifest - 1, entries, sizeof entries / sizeof entries[0]);\n\
         }\n",
    );
    source
}

/// Which entries [`isolate`] found defined.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Entries {
    pub register: bool,
    pub version: bool,
}

const SECTION_CUSTOM: u8 = 0;
const LINKING_COMDAT_INFO: u8 = 7;
const LINKING_SYMBOL_TABLE: u8 = 8;
const KIND_DATA: u8 = 1;
const KIND_SECTION: u8 = 3;
const BINDING_WEAK: u32 = 0x1;
const BINDING_LOCAL: u32 = 0x2;
const UNDEFINED: u32 = 0x10;
const EXPORTED: u32 = 0x20;
const EXPLICIT_NAME: u32 = 0x40;
const NO_STRIP: u32 = 0x80;

/// Rename the entries of a relocatable wasm object and make every other
/// defined symbol local (tool-conventions Linking.md, `linking` version 2).
/// Symbol indices are unchanged, so relocations stay valid.
pub(crate) fn isolate(object: &[u8], register: &str, version: &str) -> Result<(Vec<u8>, Entries)> {
    if object.len() < 8 || &object[..4] != b"\0asm" {
        bail!("not a wasm object");
    }
    let mut out = object[..8].to_vec();
    let mut found = Entries {
        register: false,
        version: false,
    };
    let mut linking = false;
    let mut at = 8;
    while at < object.len() {
        let id = object[at];
        let mut cursor = at + 1;
        let size = leb(object, &mut cursor)? as usize;
        let body = object
            .get(cursor..cursor + size)
            .context("truncated section")?;
        let next = cursor + size;
        if id == SECTION_CUSTOM {
            let mut c = 0;
            let name = string(body, &mut c)?;
            if name == "linking" {
                let rewritten = rewrite_linking(&body[c..], register, version, &mut found)?;
                let mut section = Vec::new();
                put_string(&mut section, "linking");
                section.extend(rewritten);
                out.push(SECTION_CUSTOM);
                put_leb(&mut out, section.len() as u64);
                out.extend(section);
                linking = true;
                at = next;
                continue;
            }
        }
        out.extend_from_slice(&object[at..next]);
        at = next;
    }
    if !linking {
        bail!("the object has no `linking` section (not a relocatable object)");
    }
    Ok((out, found))
}

fn rewrite_linking(
    body: &[u8],
    register: &str,
    version: &str,
    found: &mut Entries,
) -> Result<Vec<u8>> {
    let mut c = 0;
    let linking_version = leb(body, &mut c)?;
    if linking_version != 2 {
        bail!("unsupported `linking` section version {linking_version}");
    }
    let mut out = Vec::new();
    put_leb(&mut out, linking_version);
    while c < body.len() {
        let kind = body[c];
        c += 1;
        let size = leb(body, &mut c)? as usize;
        let payload = body
            .get(c..c + size)
            .context("truncated linking subsection")?;
        c += size;
        let payload = match kind {
            // Each addon keeps its own copy of what a COMDAT would share.
            LINKING_COMDAT_INFO => continue,
            LINKING_SYMBOL_TABLE => rewrite_symbols(payload, register, version, found)?,
            _ => payload.to_vec(),
        };
        out.push(kind);
        put_leb(&mut out, payload.len() as u64);
        out.extend(payload);
    }
    Ok(out)
}

fn rewrite_symbols(
    table: &[u8],
    register: &str,
    version: &str,
    found: &mut Entries,
) -> Result<Vec<u8>> {
    let mut c = 0;
    let count = leb(table, &mut c)?;
    let mut out = Vec::new();
    put_leb(&mut out, count);
    for _ in 0..count {
        let kind = *table.get(c).context("truncated symbol")?;
        c += 1;
        let mut flags = leb(table, &mut c)? as u32;
        let defined = flags & UNDEFINED == 0;
        // (index, name, data extent) as the encoding carries them.
        let mut index = None;
        let mut name = None;
        let mut extent = None;
        match kind {
            KIND_DATA => {
                name = Some(string(table, &mut c)?);
                if defined {
                    extent = Some((
                        leb(table, &mut c)?,
                        leb(table, &mut c)?,
                        leb(table, &mut c)?,
                    ));
                }
            }
            KIND_SECTION => index = Some(leb(table, &mut c)?),
            _ => {
                index = Some(leb(table, &mut c)?);
                if defined || flags & EXPLICIT_NAME != 0 {
                    name = Some(string(table, &mut c)?);
                }
            }
        }
        let mut name = name.map(str::to_string);
        if defined && kind != KIND_SECTION {
            match name.as_deref() {
                Some(ENTRY) => {
                    name = Some(register.to_string());
                    found.register = true;
                    flags &= !(BINDING_WEAK | BINDING_LOCAL | EXPORTED);
                }
                Some(VERSION) => {
                    name = Some(version.to_string());
                    found.version = true;
                    flags &= !(BINDING_WEAK | BINDING_LOCAL | EXPORTED);
                }
                _ => {
                    flags = (flags & !(BINDING_WEAK | EXPORTED | NO_STRIP)) | BINDING_LOCAL;
                }
            }
        }
        out.push(kind);
        put_leb(&mut out, u64::from(flags));
        if let Some(index) = index {
            put_leb(&mut out, index);
        }
        if let Some(name) = &name {
            put_string(&mut out, name);
        }
        if let Some((segment, offset, size)) = extent {
            put_leb(&mut out, segment);
            put_leb(&mut out, offset);
            put_leb(&mut out, size);
        }
    }
    if c != table.len() {
        bail!("trailing bytes after the symbol table");
    }
    Ok(out)
}

fn leb(bytes: &[u8], at: &mut usize) -> Result<u64> {
    let mut value = 0u64;
    let mut shift = 0;
    loop {
        let byte = *bytes.get(*at).context("truncated LEB128")?;
        *at += 1;
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Ok(value);
        }
        shift += 7;
        if shift > 63 {
            bail!("LEB128 overflow");
        }
    }
}

fn put_leb(out: &mut Vec<u8>, mut value: u64) {
    loop {
        let byte = (value & 0x7f) as u8;
        value >>= 7;
        if value == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

fn string<'a>(bytes: &'a [u8], at: &mut usize) -> Result<&'a str> {
    let len = leb(bytes, at)? as usize;
    let text = bytes.get(*at..*at + len).context("truncated name")?;
    *at += len;
    std::str::from_utf8(text).context("symbol name is not UTF-8")
}

fn put_string(out: &mut Vec<u8>, text: &str) {
    put_leb(out, text.len() as u64);
    out.extend_from_slice(text.as_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal relocatable: a symbol table with the entry, a helper, an
    /// undefined import and a data symbol, plus a COMDAT subsection.
    fn object() -> Vec<u8> {
        let mut symbols = Vec::new();
        put_leb(&mut symbols, 4);
        // function 0: napi_register_wasm_v1 (defined, global)
        symbols.push(0);
        put_leb(&mut symbols, 0);
        put_leb(&mut symbols, 0);
        put_string(&mut symbols, ENTRY);
        // function 1: helper (defined, weak + hidden)
        symbols.push(0);
        put_leb(&mut symbols, 0x5);
        put_leb(&mut symbols, 1);
        put_string(&mut symbols, "helper");
        // function 2: napi_create_function (undefined, no explicit name)
        symbols.push(0);
        put_leb(&mut symbols, u64::from(UNDEFINED));
        put_leb(&mut symbols, 2);
        // data: table_of_things (segment 0, offset 4, size 8)
        symbols.push(KIND_DATA);
        put_leb(&mut symbols, 0);
        put_string(&mut symbols, "table_of_things");
        put_leb(&mut symbols, 0);
        put_leb(&mut symbols, 4);
        put_leb(&mut symbols, 8);
        let mut linking = Vec::new();
        put_string(&mut linking, "linking");
        put_leb(&mut linking, 2);
        linking.push(LINKING_COMDAT_INFO);
        put_leb(&mut linking, 1);
        linking.push(0);
        linking.push(LINKING_SYMBOL_TABLE);
        put_leb(&mut linking, symbols.len() as u64);
        linking.extend(symbols);
        let mut out = b"\0asm\x01\0\0\0".to_vec();
        out.push(SECTION_CUSTOM);
        put_leb(&mut out, linking.len() as u64);
        out.extend(linking);
        out
    }

    fn symbols_of(object: &[u8]) -> Vec<(u32, Option<String>)> {
        let mut at = 8;
        let _ = object[at];
        at += 1;
        let _size = leb(object, &mut at).unwrap();
        let name = string(object, &mut at).unwrap();
        assert_eq!(name, "linking");
        assert_eq!(leb(object, &mut at).unwrap(), 2);
        let mut result = Vec::new();
        while at < object.len() {
            let kind = object[at];
            at += 1;
            let size = leb(object, &mut at).unwrap() as usize;
            assert_ne!(kind, LINKING_COMDAT_INFO, "COMDAT groups must be dropped");
            if kind == LINKING_SYMBOL_TABLE {
                let table = &object[at..at + size];
                let mut c = 0;
                for _ in 0..leb(table, &mut c).unwrap() {
                    let kind = table[c];
                    c += 1;
                    let flags = leb(table, &mut c).unwrap() as u32;
                    let mut name = None;
                    if kind == KIND_DATA {
                        name = Some(string(table, &mut c).unwrap().to_string());
                        for _ in 0..3 {
                            leb(table, &mut c).unwrap();
                        }
                    } else {
                        leb(table, &mut c).unwrap();
                        if flags & UNDEFINED == 0 {
                            name = Some(string(table, &mut c).unwrap().to_string());
                        }
                    }
                    result.push((flags, name));
                }
            }
            at += size;
        }
        result
    }

    #[test]
    fn entry_is_renamed_and_everything_else_defined_is_local() {
        let (out, found) = isolate(
            &object(),
            "__perry_addon_x_register",
            "__perry_addon_x_version",
        )
        .unwrap();
        assert_eq!(
            found,
            Entries {
                register: true,
                version: false
            }
        );
        let symbols = symbols_of(&out);
        assert_eq!(
            symbols[0],
            (0, Some("__perry_addon_x_register".to_string()))
        );
        assert_eq!(
            symbols[1],
            (BINDING_LOCAL | 0x4, Some("helper".to_string()))
        );
        assert_eq!(
            symbols[2],
            (UNDEFINED, None),
            "undefined imports stay global"
        );
        assert_eq!(
            symbols[3],
            (BINDING_LOCAL, Some("table_of_things".to_string()))
        );
    }

    #[test]
    fn a_native_object_is_refused() {
        assert!(isolate(b"\x7fELF\x02\x01\x01\0", "a", "b").is_err());
    }

    #[test]
    fn registration_lists_entries_in_manifest_order() {
        let source = registration_source(
            "{}",
            &[
                ("a_register".into(), None),
                ("b_register".into(), Some("b_version".into())),
            ],
        );
        let a = source.find("{ a_register, NULL }").unwrap();
        let b = source.find("{ b_register, b_version }").unwrap();
        assert!(a < b);
        assert!(source.contains("__attribute__((constructor))"));
    }
}
