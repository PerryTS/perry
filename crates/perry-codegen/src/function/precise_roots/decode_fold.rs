//! One `add` for a root reload that only derives an interior address.
//!
//! An `i64` root home holds a raw GC pointer in codegen's view, and the
//! lowering in the parent module stores it as a JSValue word at rest:
//! `or value, POINTER_TAG` on the way in, `and word, POINTER_MASK` on every
//! reload. Most reloads of such a home exist only to address a field of the
//! object — a scope object's binding slot, a closure's capture word — so the
//! decoded value has exactly one use, `add decoded, C`, which then feeds an
//! `inttoptr`.
//!
//! For a word the store side produced, `word = POINTER_TAG + addr` (the tag
//! and the 48-bit address occupy disjoint bits), so
//! `(word & POINTER_MASK) + C == word + (C - POINTER_TAG)` in wrapping `i64`
//! arithmetic. The decode and the displacement are therefore one `add` with
//! the tag folded into the constant: the same address, one instruction
//! instead of two, at every such site.
//!
//! Only a decode whose single use is the immediately following constant
//! `add` is fused. Any other use — a null test, a write-barrier parent, a
//! re-encode, a second address — keeps the masked value, so the fusion never
//! changes what a value that is still observed as a pointer looks like. A
//! null home has no object to address; deriving a field address from it is
//! already invalid before this fusion.

use super::is_local_name_char;

/// The constant the parent lowering masks every `i64`-home reload with.
const DECODE_MASK: &str = "281474976710655";

/// `%r` when `line` is the parent lowering's reload decode
/// `%r = and i64 %r.rs4o, 281474976710655`.
fn decode_result(line: &str) -> Option<&str> {
    let (result, rhs) = line.trim_start().split_once(" = and i64 ")?;
    let rest = rhs.strip_prefix(result)?.strip_prefix(".rs4o, ")?;
    (rest.trim_end() == DECODE_MASK).then_some(result)
}

/// `(%y, C)` when `line` is `%y = add i64 <decoded>, C` with a literal `C`.
fn constant_offset<'a>(line: &'a str, decoded: &str) -> Option<(&'a str, i64)> {
    let (result, rhs) = line.trim_start().split_once(" = add i64 ")?;
    let offset = rhs.strip_prefix(decoded)?.strip_prefix(", ")?;
    Some((result, offset.trim_end().parse().ok()?))
}

/// Fuse each single-use reload decode into the constant `add` that follows
/// it. Linear in the function text: one pass names the decodes, one counts
/// their mentions, one rewrites.
pub(super) fn fuse_reload_decode_offsets(ir: &str) -> String {
    let lines: Vec<&str> = ir.lines().collect();
    let mut mentions: std::collections::HashMap<&str, u32> = lines
        .iter()
        .filter_map(|line| decode_result(line))
        .map(|result| (result, 0))
        .collect();
    if mentions.is_empty() {
        return ir.to_string();
    }
    for line in &lines {
        for token in line.split(|c: char| !is_local_name_char(c)) {
            if let Some(count) = mentions.get_mut(token) {
                *count += 1;
            }
        }
    }
    let tag = crate::nanbox::POINTER_TAG as i64;
    let mut out = String::with_capacity(ir.len());
    let mut index = 0;
    while index < lines.len() {
        let line = lines[index];
        // Two mentions: the decode's own definition and its one use.
        let fused = decode_result(line)
            .filter(|decoded| mentions.get(decoded) == Some(&2))
            .and_then(|decoded| {
                let next = lines.get(index + 1)?;
                let (result, offset) = constant_offset(next, decoded)?;
                Some(format!(
                    "  {result} = add i64 {decoded}.rs4o, {}\n",
                    offset.wrapping_sub(tag)
                ))
            });
        if let Some(fused) = fused {
            out.push_str(&fused);
            index += 2;
            continue;
        }
        out.push_str(line);
        out.push('\n');
        index += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_mask_is_the_pointer_mask() {
        assert_eq!(DECODE_MASK, crate::nanbox::POINTER_MASK.to_string());
    }

    /// The fused constant addresses the same byte as mask-then-add for every
    /// word the store side can produce.
    #[test]
    fn fused_offset_addresses_the_same_field() {
        let tag = crate::nanbox::POINTER_TAG;
        for addr in [
            0x1000u64,
            0x7f12_3456_7890,
            crate::nanbox::POINTER_MASK & !7,
        ] {
            for offset in [0i64, 8, 944, 474_256] {
                let word = addr | tag;
                let masked = (word & crate::nanbox::POINTER_MASK).wrapping_add(offset as u64);
                let fused = word.wrapping_add(offset.wrapping_sub(tag as i64) as u64);
                assert_eq!(masked, fused, "addr {addr:#x} offset {offset}");
            }
        }
    }

    const SINGLE_USE: &str = "  %r1.rs4p = load ptr addrspace(1), ptr %s\n  %r1.rs4i = ptrtoint ptr addrspace(1) %r1.rs4p to i64\n  %r1.rs4o = call i64 asm \"\", \"=r,0\"(i64 %r1.rs4i) \"gc-leaf-function\"\n  %r1 = and i64 %r1.rs4o, 281474976710655\n  %r2 = add i64 %r1, 944\n  %r3 = inttoptr i64 %r2 to ptr\n";

    #[test]
    fn a_single_use_decode_joins_its_offset() {
        let fused = fuse_reload_decode_offsets(SINGLE_USE);
        let expected = 944i64.wrapping_sub(crate::nanbox::POINTER_TAG as i64);
        assert!(
            fused.contains(&format!("  %r2 = add i64 %r1.rs4o, {expected}\n")),
            "{fused}"
        );
        assert!(!fused.contains(" = and i64 "), "{fused}");
        assert!(fused.contains("%r3 = inttoptr i64 %r2 to ptr"), "{fused}");
    }

    /// A decode that is also observed as a pointer (here the write-barrier
    /// parent) keeps its mask, and so does one whose use is not an offset.
    #[test]
    fn a_decode_with_another_use_keeps_its_mask() {
        let shared = format!("{SINGLE_USE}  call void @js_write_barrier(i64 %r1, i64 %v)\n");
        assert_eq!(fuse_reload_decode_offsets(&shared), shared);
        let test = SINGLE_USE.replace("%r2 = add i64 %r1, 944", "%r2 = icmp eq i64 %r1, 0");
        assert_eq!(fuse_reload_decode_offsets(&test), test);
        let register = SINGLE_USE.replace("add i64 %r1, 944", "add i64 %r1, %k");
        assert_eq!(fuse_reload_decode_offsets(&register), register);
    }
}
