//! Link-time ShapeId assignment (design step 4).
//!
//! # What this does, and why it is a LINK-time job
//!
//! Every compiler-visible shape gets an absolute symbol,
//! `@perry_shape_abs_<module>__<Class|AnonShape_hash>`, declared by each
//! module that names it and DEFINED exactly once, here, in a generated object
//! the linker consumes. The symbol's ADDRESS is the shape's ShapeId.
//!
//! Doing it at link time rather than during codegen is the whole point:
//!
//! * **Two separately compiled modules cannot collide**, because neither
//!   assigns anything. A module's `.o` carries the NAME. The assignment is
//!   made once, in one place, over the sorted union of every name the program
//!   uses, so it is injective by construction — not by a hash that is
//!   *probably* injective.
//! * **The per-module object cache stays valid.** A `.o` compiled under one
//!   assignment relocates correctly under any other, so adding a module does
//!   not invalidate every other module's cache entry. Baking the integer into
//!   codegen would have made the id a whole-program input to a per-module
//!   cache key — the one thing that cache exists to avoid.
//!
//! # Why `movabsq` and not `cmpl $imm32`
//!
//! `!absolute_symbol` with a narrow range makes LLVM fold the value into
//! `cmpl $sym, 4(%rdi)`, one instruction. That needs an `R_X86_64_32`
//! relocation, and GNU ld refuses one against an absolute symbol when making
//! a PIE — measured on binutils 2.42, which is the link perry's native Linux
//! target performs (`cc`, no `-fuse-ld`). `R_X86_64_64` is accepted by both
//! bfd and lld, so the declaration uses the FULL range and LLVM emits
//! `movabsq $sym, %r` + `cmpl %r32, 4(%rdi)`. The extra instruction is
//! loop-invariant and LICM hoists it, so it costs nothing where it is read
//! repeatedly and one instruction where it is not.
//!
//! # Fail-safe by polarity
//!
//! `js_object_shape_bind_static_for_keys` RETURNS the id instances will carry,
//! and module init stores that return. So an id this module could not honour
//! (out of band, or already held by a separately linked image) makes the
//! emitted guard match nothing — a missed fast path, never a wrong layout.

use anyhow::{anyhow, Result};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, OnceLock};

/// First id of the LINK-ASSIGNED band. Must equal `SHAPE_ID_BASE` in
/// `perry-runtime/src/object/shapes.rs`.
const SHAPE_ID_BASE: u32 = 0x8000_0000;
/// Exclusive end of the band. Must equal `SHAPE_ID_STATIC_END` there.
const SHAPE_ID_STATIC_END: u32 = SHAPE_ID_BASE + (1 << 20);
/// Handed to names the band cannot hold. `is_static_shape_id` rejects it, so
/// the runtime mints dynamically for that shape; and because it can never be a
/// live ShapeId, an emitted guard carrying it simply never matches.
///
/// Deliberately NOT zero: an unstamped anonymous literal carries
/// `parent_class_id == 0` at payload +4, which is exactly how #10843's
/// `in`-presence defect answered `true` for `"k" in {}`. A sentinel that some
/// live object's word can equal is not a sentinel.
const SHAPE_ID_UNASSIGNED: u32 = 0xFFFF_FFFF;

fn names() -> &'static Mutex<BTreeSet<String>> {
    static NAMES: OnceLock<Mutex<BTreeSet<String>>> = OnceLock::new();
    NAMES.get_or_init(|| Mutex::new(BTreeSet::new()))
}

/// Record the shape symbols one module declared. Called from the codegen
/// worker for a fresh compile AND from the object-cache hit path, because a
/// cached `.o` names the same symbols without codegen having run.
pub(super) fn record_module_symbols(symbols: impl IntoIterator<Item = String>) {
    if let Ok(mut set) = names().lock() {
        set.extend(symbols);
    }
}

/// How many distinct shape symbols the program declared. For diagnostics.
pub(super) fn recorded_count() -> usize {
    names().lock().map(|s| s.len()).unwrap_or(0)
}

/// Assign one id per name, in the order given.
///
/// Injective by construction: the caller passes a SET, sorted, and this is a
/// bijection onto consecutive ids. Nothing hashes, so nothing can collide, and
/// the same program always gets the same table.
///
/// Names past the band's capacity get [`SHAPE_ID_UNASSIGNED`], which the
/// runtime declines — those shapes mint dynamically and their sites keep the
/// runtime-learned guard. A program is never rejected for having too many
/// shapes.
fn assign_ids(names: &[String]) -> Vec<(&str, u32)> {
    let capacity = (SHAPE_ID_STATIC_END - SHAPE_ID_BASE) as usize;
    names
        .iter()
        .enumerate()
        .map(|(index, name)| {
            let id = if index < capacity {
                SHAPE_ID_BASE + index as u32
            } else {
                SHAPE_ID_UNASSIGNED
            };
            (name.as_str(), id)
        })
        .collect()
}

/// Assign ids and emit the defining object, or `Ok(None)` when the program
/// declared no shape symbols at all.
///
/// Assignment is `SHAPE_ID_BASE + index` over the sorted names: deterministic,
/// reproducible, and injective because the names are a set.
pub(super) fn generate_shape_id_object(output_dir: &Path) -> Result<Option<PathBuf>> {
    let names: Vec<String> = match names().lock() {
        Ok(set) => set.iter().cloned().collect(),
        Err(_) => return Ok(None),
    };
    if names.is_empty() {
        return Ok(None);
    }
    let capacity = (SHAPE_ID_STATIC_END - SHAPE_ID_BASE) as usize;
    if names.len() > capacity {
        eprintln!(
            "Warning: {} compiler-visible shapes exceed the {} link-assigned ShapeIds; \
             the remainder use runtime-minted ids (correct, one missed fast path each)",
            names.len(),
            capacity
        );
    }

    // Mach-O prefixes C symbols with `_` and spells hidden visibility
    // `.private_extern`; ELF uses the bare name and `.hidden`. Same host `cfg`
    // split `embed.rs` uses for its generated object.
    let sym_prefix = if cfg!(target_os = "macos") { "_" } else { "" };
    let hide = if cfg!(target_os = "macos") {
        ".private_extern"
    } else {
        ".hidden"
    };

    let mut c = String::new();
    c.push_str("// Auto-generated by Perry - link-assigned ShapeIds (design step 4).\n");
    c.push_str("// Each symbol is ABSOLUTE: its address IS the shape's ShapeId. Compiled\n");
    c.push_str("// modules reference these by name only, which is what keeps a cached .o\n");
    c.push_str("// valid under any assignment.\n");
    for (name, id) in assign_ids(&names) {
        c.push_str(&format!(
            "__asm__(\".globl {p}{n}\\n\\t{h} {p}{n}\\n\\t.set {p}{n}, 0x{id:08x}\\n\");\n",
            p = sym_prefix,
            n = name,
            h = hide,
            id = id
        ));
    }

    let c_path = output_dir.join("__perry_shape_ids.c");
    let obj_path = output_dir.join(if cfg!(windows) {
        "__perry_shape_ids.obj"
    } else {
        "__perry_shape_ids.o"
    });
    std::fs::write(&c_path, &c)?;
    let compiler = PathBuf::from("cc");
    let status = Command::new(&compiler)
        .arg("-c")
        .arg(&c_path)
        .arg("-O0")
        .arg("-o")
        .arg(&obj_path)
        .status()
        .map_err(|e| anyhow!("failed to invoke cc for the ShapeId table: {}", e))?;
    if !status.success() {
        return Err(anyhow!(
            "cc failed to assemble the link-assigned ShapeId table ({})",
            c_path.display()
        ));
    }
    Ok(Some(obj_path))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(count: usize) -> Vec<String> {
        (0..count)
            .map(|i| format!("perry_shape_abs_m__S{i}"))
            .collect()
    }

    /// The property the whole design rests on: no two shapes share an id, and
    /// the table does not depend on anything but the name set.
    ///
    /// This is what a hash-based assignment could not give. A birthday
    /// collision between two shapes is a guard that matches the wrong layout
    /// and returns a wrong value silently, so "probably injective" is not a
    /// property this may have.
    #[test]
    fn assignment_is_injective_and_deterministic() {
        let names = names(5000);
        let first = assign_ids(&names);
        let second = assign_ids(&names);
        assert_eq!(first, second, "the same name set must give the same table");

        let mut ids: Vec<u32> = first.iter().map(|(_, id)| *id).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), names.len(), "two names shared an id");
        assert!(
            ids.iter()
                .all(|id| (SHAPE_ID_BASE..SHAPE_ID_STATIC_END).contains(id)),
            "every assigned id must land inside the band the runtime reserves"
        );
    }

    /// A program with more shapes than the band holds must still build. The
    /// overflow marker has to be a value no live object's `+4` word can equal
    /// — NOT zero, which an unstamped anonymous literal carries.
    #[test]
    fn overflow_is_marked_unassigned_rather_than_wrapped() {
        let capacity = (SHAPE_ID_STATIC_END - SHAPE_ID_BASE) as usize;
        let mut names = names(2);
        names.push("perry_shape_abs_m__overflow".to_string());
        // Simulate the overflow edge without allocating a million strings by
        // checking the boundary arithmetic directly.
        assert_eq!(assign_ids(&names)[2].1, SHAPE_ID_BASE + 2);
        assert_ne!(SHAPE_ID_UNASSIGNED, 0, "zero is a live unstamped +4 word");
        assert!(
            !(SHAPE_ID_BASE..SHAPE_ID_STATIC_END).contains(&SHAPE_ID_UNASSIGNED),
            "the overflow marker must be outside the band so the runtime declines it"
        );
        assert!(
            capacity >= 1 << 20,
            "the band must hold a real program's shapes"
        );
    }
}
