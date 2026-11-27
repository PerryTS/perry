//! Named reads of a typed array's metadata and prototype data, answered from
//! the cell's structure: one header-word kind test, the link word, then a load.
//!
//! A typed array (`GC_TYPE` `0x40 | brand`, a view `0x60 | brand`, brand
//! 0..=11) keeps every own string-keyed property and a non-default
//! `[[Prototype]]` in its property bag, which its link word names. A cell
//! whose link is null (an owner) or names its owning byte cell directly (a
//! view) therefore owns nothing but its indices and inherits from its kind's
//! intrinsic prototype `K.prototype`. For such a cell:
//!
//! * `length`, `byteLength`, `byteOffset` are `%TypedArray%.prototype`'s
//!   accessors while `PERRY_TYPED_NAMED_PROPS_INVALIDATED` is clear (the
//!   runtime sets it, sticky, before any own definition, deletion or store of
//!   one of those names on a typed-array prototype publishes, and before a user
//!   relinks one): the owner's live length word (detach rewrites it), a fixed
//!   view's length and offset words while its owner is neither resizable nor
//!   detached, `byteLength` = `length << log2(BYTES_PER_ELEMENT)`;
//! * `BYTES_PER_ELEMENT` is `K.prototype`'s own data property, neither
//!   writable nor configurable (ECMA-262 23.2.7.2): the kind's element size,
//!   with no proof beyond the cell's;
//! * `constructor` is whatever `K.prototype`'s chain holds: the runtime leaf
//!   `perry_typed_array_proto_get` probes `K.prototype` itself (its data
//!   answers, an accessor declines).
//!
//! Every other receiver, and every cell that fails a test, takes the caller's
//! complete read.

use super::*;

/// The i1 "the default typed-view metadata accessors still hold" test
/// (`PERRY_TYPED_NAMED_PROPS_INVALIDATED` clear). GUARDS1: when the whole
/// program never writes those accessors on a typed-array or Buffer prototype
/// nor relinks one (wpfacts `typed_metadata_written`, no interpreted code),
/// the runtime never sets the word and the answer is the constant `true`: no
/// load is emitted.
pub(crate) fn emit_named_props_pristine(ctx: &mut FnCtx<'_>) -> String {
    if ctx.wpfacts.typed_metadata_unwritten {
        crate::verify_facts::consumed(crate::verify_facts::Fact::IndexedMetadata, || {
            serde_json::json!({"typed_metadata":"unwritten"})
        });
        return "true".to_string();
    }
    let blk = ctx.block();
    let invalidated = blk.load_atomic_acquire(I8, "@PERRY_TYPED_NAMED_PROPS_INVALIDATED", 1);
    blk.icmp_eq(I8, &invalidated, "0")
}

/// The names this arm answers.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum TypedNamed {
    Length,
    ByteLength,
    ByteOffset,
    BytesPerElement,
    Constructor,
}

impl TypedNamed {
    pub(crate) fn of(name: &str) -> Option<Self> {
        Some(match name {
            "length" => Self::Length,
            "byteLength" => Self::ByteLength,
            "byteOffset" => Self::ByteOffset,
            "BYTES_PER_ELEMENT" => Self::BytesPerElement,
            "constructor" => Self::Constructor,
            _ => return None,
        })
    }

    /// `%TypedArray%.prototype` accessors, behind the runtime's withdrawal flag.
    fn is_accessor(self) -> bool {
        matches!(self, Self::Length | Self::ByteLength | Self::ByteOffset)
    }
}

/// `log2(BYTES_PER_ELEMENT)` of brand `b` is nibble `b` of this word
/// (`runtime_abi::BYTES_ELEMENT_SHIFT`).
fn shift_nibbles() -> u64 {
    crate::runtime_abi::BYTES_ELEMENT_SHIFT[..12]
        .iter()
        .enumerate()
        .fold(0u64, |p, (i, s)| p | ((*s as u64) << (i * 4)))
}

/// The key operands a `constructor` arm hands its leaf: the site's shape
/// directory, the key word and its compile-time hash.
pub(crate) struct ProtoProbe<'a> {
    pub(crate) dir: &'a str,
    pub(crate) key_bits: &'a str,
    pub(crate) hash: &'a str,
}

/// Emit the arm in the CURRENT block. `handle` is the payload of a
/// POINTER-tagged receiver above the native-handle band (the caller's
/// receiver test). A typed array this arm answers branches to `merge_label`
/// with the returned `(value, end_label)`; every other receiver branches to
/// `miss_label`. `probe` is required for [`TypedNamed::Constructor`].
pub(crate) fn emit_typed_named_arm(
    ctx: &mut FnCtx<'_>,
    handle: &str,
    which: TypedNamed,
    probe: Option<ProtoProbe<'_>>,
    miss_label: &str,
    merge_label: &str,
) -> (String, String) {
    let link_idx = ctx.new_block("tnamed.link");
    let view_idx = ctx.new_block("tnamed.view");
    let view_owner_idx = ctx.new_block("tnamed.view_owner");
    let ok_idx = ctx.new_block("tnamed.ok");
    let link_label = ctx.block_label(link_idx);
    let view_label = ctx.block_label(view_idx);
    let view_owner_label = ctx.block_label(view_owner_idx);
    let ok_label = ctx.block_label(ok_idx);

    // The kind test: type byte (view bit cleared) in 0x40..=0x4b and the
    // FORWARDED flag (bit 7 of the flags byte) clear, in one compare.
    let hw = crate::expr::byte_cell::header_word(ctx.block(), handle);
    let blk = ctx.block();
    let masked = blk.and(I64, &hw, "32991"); // 0x80DF
    let brand = blk.sub(I64, &masked, "64");
    let is_typed = blk.icmp_ule(I64, &brand, "11");
    blk.cond_br(&is_typed, &link_label, miss_label);

    // An owner with a null link has no bag: no own property, the default chain.
    ctx.current_block = link_idx;
    let blk = ctx.block();
    let link_addr = blk.add(I64, handle, &crate::runtime_abi::BYTES_LINK.to_string());
    let link_ptr = blk.inttoptr(I64, &link_addr);
    let link = blk.load(I64, &link_ptr);
    let view_bit = blk.and(I64, &hw, &crate::runtime_abi::BYTES_TYPE_VIEW.to_string());
    let is_view = blk.icmp_ne(I64, &view_bit, "0");
    let no_link = blk.icmp_eq(I64, &link, "0");
    let not_view = blk.icmp_eq(I64, &view_bit, "0");
    let owner_ok = blk.and(I1, &not_view, &no_link);
    blk.cond_br(&owner_ok, &ok_label, &view_label);

    // A fixed-length view linked straight to its owner (no bag).
    ctx.current_block = view_idx;
    let blk = ctx.block();
    let tracking = blk.and(I64, &hw, &(1u64 << 23).to_string());
    let fixed = blk.icmp_eq(I64, &tracking, "0");
    let has_link = blk.icmp_ne(I64, &link, "0");
    let view_ok = blk.and(I1, &is_view, &fixed);
    let view_ok = blk.and(I1, &view_ok, &has_link);
    blk.cond_br(&view_ok, &view_owner_label, miss_label);

    // The link is a plain owner byte cell (not a bag, not a view), neither
    // RESIZABLE nor DETACHED: only those change a fixed view's extent.
    ctx.current_block = view_owner_idx;
    let blk = ctx.block();
    let owner_word = crate::expr::byte_cell::header_word(blk, &link);
    let mask = 0xe0u64 | (1 << 24) | (1 << 30);
    let state = blk.and(I64, &owner_word, &mask.to_string());
    let plain_owner = blk.icmp_eq(
        I64,
        &state,
        &crate::runtime_abi::BYTES_TYPE_BASE.to_string(),
    );
    blk.cond_br(&plain_owner, &ok_label, miss_label);

    ctx.current_block = ok_idx;
    if which.is_accessor() {
        let hit_idx = ctx.new_block("tnamed.hit");
        let hit_label = ctx.block_label(hit_idx);
        let pristine = emit_named_props_pristine(ctx);
        ctx.block().cond_br(&pristine, &hit_label, miss_label);
        ctx.current_block = hit_idx;
    }
    // log2(BYTES_PER_ELEMENT): nibble `brand` of `shift_nibbles()`.
    let shift = |blk: &mut crate::block::LlBlock| {
        let at = blk.shl(I64, &brand, "2");
        let s = blk.lshr(I64, &shift_nibbles().to_string(), &at);
        blk.and(I64, &s, "15")
    };
    let value = match which {
        TypedNamed::Length => {
            let blk = ctx.block();
            let ptr = blk.inttoptr(I64, handle);
            let len = blk.load(I32, &ptr);
            blk.uitofp(I32, &len, DOUBLE)
        }
        TypedNamed::ByteLength => {
            let blk = ctx.block();
            let ptr = blk.inttoptr(I64, handle);
            let len = blk.load(I32, &ptr);
            let len = blk.zext(I32, &len, I64);
            let s = shift(blk);
            let bytes = blk.shl(I64, &len, &s);
            blk.uitofp(I64, &bytes, DOUBLE)
        }
        TypedNamed::ByteOffset => {
            let blk = ctx.block();
            let aux_addr = blk.add(I64, handle, &crate::runtime_abi::BYTES_AUX.to_string());
            let aux_ptr = blk.inttoptr(I64, &aux_addr);
            let aux = blk.load(I32, &aux_ptr);
            let offset = blk.select(I1, &is_view, I32, &aux, "0");
            blk.uitofp(I32, &offset, DOUBLE)
        }
        TypedNamed::BytesPerElement => {
            let blk = ctx.block();
            let s = shift(blk);
            let size = blk.shl(I64, "1", &s);
            blk.uitofp(I64, &size, DOUBLE)
        }
        TypedNamed::Constructor => {
            let probe = probe.expect("a constructor arm probes K.prototype");
            let value = ctx.block().call(
                DOUBLE,
                "perry_typed_array_proto_get",
                &[
                    (PTR, probe.dir),
                    (I64, handle),
                    (I64, probe.key_bits),
                    (I64, probe.hash),
                ],
            );
            let blk = ctx.block();
            let bits = blk.bitcast_double_to_i64(&value);
            let served = blk.icmp_ne(I64, &bits, crate::nanbox::TAG_HOLE_I64);
            let served_idx = ctx.new_block("tnamed.served");
            let served_label = ctx.block_label(served_idx);
            ctx.block().cond_br(&served, &served_label, miss_label);
            ctx.current_block = served_idx;
            value
        }
    };
    let end = ctx.block().label.clone();
    ctx.block().br(merge_label);
    (value, end)
}
