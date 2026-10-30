//! GC type-metadata verification, split out of `gc::types` to keep that file
//! under the 2000-line cap. The parent re-exports both entry points, so every
//! call site and the `gc/tests` verification keep their `gc::types::validate_*`
//! path.
use super::*;

// #854: part of GC type-metadata verification contract (exercised by gc/tests)
#[allow(dead_code)]
pub(crate) fn validate_gc_type_info(info: &GcTypeInfo) -> Result<(), &'static str> {
    let descriptor_is_leaf = info.rewrite_descriptor_kind == GcRewriteDescriptorKind::Leaf;
    if info.pointer_free {
        if !descriptor_is_leaf {
            return Err("pointer-free GC type exposes a rewrite descriptor");
        }
        if info.layout_slot_kind != GcLayoutSlotKind::None {
            return Err("pointer-free GC type exposes pointer slots");
        }
        return Ok(());
    }

    if descriptor_is_leaf {
        return Err("pointerful GC type lacks rewrite descriptor metadata");
    }

    match info.rewrite_descriptor_kind {
        GcRewriteDescriptorKind::Array => {
            if info.layout_slot_kind != GcLayoutSlotKind::ArrayElements {
                return Err("array rewrite descriptor must expose array element slots");
            }
        }
        GcRewriteDescriptorKind::Object => {
            if info.layout_slot_kind != GcLayoutSlotKind::ObjectFields {
                return Err("object rewrite descriptor must expose object field slots");
            }
        }
        GcRewriteDescriptorKind::RegExp => {
            if info.layout_slot_kind != GcLayoutSlotKind::RegExpFields {
                return Err("regexp rewrite descriptor must expose regexp fields");
            }
        }
        GcRewriteDescriptorKind::Closure => {
            if info.layout_slot_kind != GcLayoutSlotKind::ClosureCaptures {
                return Err("closure rewrite descriptor must expose closure capture slots");
            }
        }
        GcRewriteDescriptorKind::WeakStorage
        | GcRewriteDescriptorKind::Overflow
        | GcRewriteDescriptorKind::Box
        | GcRewriteDescriptorKind::Scope
        | GcRewriteDescriptorKind::Buffer
        | GcRewriteDescriptorKind::MetaOnly
        | GcRewriteDescriptorKind::RegexProgram
        | GcRewriteDescriptorKind::NativeHandle
        | GcRewriteDescriptorKind::Promise
        | GcRewriteDescriptorKind::AsyncSnapshot
        | GcRewriteDescriptorKind::AsyncResourceState
        | GcRewriteDescriptorKind::Symbol
        | GcRewriteDescriptorKind::Error
        | GcRewriteDescriptorKind::Map
        | GcRewriteDescriptorKind::MapStore
        | GcRewriteDescriptorKind::LazyArray
        | GcRewriteDescriptorKind::Set
        | GcRewriteDescriptorKind::SetStore
        | GcRewriteDescriptorKind::Proxy => {
            if info.layout_slot_kind != GcLayoutSlotKind::None {
                return Err(
                    "external-backed rewrite descriptor must not expose payload layout slots",
                );
            }
        }
        GcRewriteDescriptorKind::ObjectMeta => {
            // Meta records expose every child edge (prototype, spill buffer,
            // and private-evaluation brand) to marking and rewriting.
            if info.layout_slot_kind != GcLayoutSlotKind::ObjectMeta {
                return Err("object-meta descriptor must expose its child edges to marking");
            }
        }
        GcRewriteDescriptorKind::NativePodView => {
            if info.layout_slot_kind != GcLayoutSlotKind::None {
                return Err("native view rewrite descriptor must use fixed slots only");
            }
        }
        GcRewriteDescriptorKind::Leaf => unreachable!("leaf handled above"),
    }

    Ok(())
}

// #854: part of GC type-metadata verification contract (exercised by gc/tests)
#[allow(dead_code)]
pub(crate) fn validate_gc_type_metadata() -> Result<(), String> {
    for info in gc_type_infos() {
        validate_gc_type_info(info)
            .map_err(|reason| format!("invalid GC metadata for {}: {}", info.name, reason))?;
    }
    Ok(())
}
