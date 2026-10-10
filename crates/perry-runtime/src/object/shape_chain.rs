//! A holder entry's proof of the links it consumed. Owned by that entry;
//! never indexed independently by a key, receiver, or class.
use crate::object::ObjectHeader;

#[repr(C)]
pub(crate) struct ShapeHop {
    addr: usize,
    word: u64,
    /// Identity-word slots are stable metadata; object addresses are GC edges.
    heap: bool,
}

#[repr(C)]
pub(crate) struct ShapeChain {
    hops: *mut ShapeHop,
    len: usize,
}

impl Drop for ShapeChain {
    fn drop(&mut self) {
        unsafe {
            drop(Box::from_raw(std::ptr::slice_from_raw_parts_mut(
                self.hops, self.len,
            )));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_shape_chain_layout_matches_the_emitted_loads() {
        assert_eq!(std::mem::offset_of!(ShapeChain, hops), 0);
        assert_eq!(
            std::mem::offset_of!(ShapeChain, len),
            std::mem::size_of::<usize>()
        );
        assert_eq!(
            std::mem::size_of::<ShapeHop>(),
            crate::codegen_abi::SHAPE_CHAIN_HOP_BYTES
        );
        assert_eq!(std::mem::offset_of!(ShapeHop, addr), 0);
        assert_eq!(std::mem::offset_of!(ShapeHop, word), 8);
        #[cfg(target_pointer_width = "64")]
        assert_eq!(
            std::mem::offset_of!(ShapeChain, len),
            crate::codegen_abi::SHAPE_CHAIN_LEN_OFFSET
        );
    }

    #[test]
    fn class_shape_chain_guards_the_live_identity_word() {
        let _lock = crate::gc::global_side_table_test_lock();
        let _no_move = crate::gc::GcSuppressScope::new();
        crate::object::builtin_prototype_value("Object");
        unsafe {
            let cid = 0x4323_1000;
            crate::object::js_register_class_name(cid, b"ChainIdentity".as_ptr(), 13);
            let holder = crate::value::js_nanbox_get_pointer(
                crate::object::class_registry::class_decl_prototype_value(cid),
            ) as *mut ObjectHeader;
            let recv = crate::object::js_object_alloc(cid, 0);
            let mut proof = ShapeChain::capture(recv).expect("class chain admitted");
            assert!(proof.valid());
            let mut traced = Vec::new();
            proof.scan(&mut crate::gc::RuntimeRootVisitor::for_copy(&mut |v| {
                traced.push(v.to_bits() & crate::value::POINTER_MASK);
            }));
            assert!(
                traced.iter().filter(|&&addr| addr == holder as u64).count() >= 2,
                "both the saved identity answer and object hop are rooted"
            );
            crate::object::shapes::write_identity_word(
                crate::object::shapes::class_identity_proto_id(cid),
                crate::value::TAG_UNDEFINED,
            );
            assert!(
                !proof.valid(),
                "replacing a CLASS holder invalidates the proof"
            );
        }
    }

    #[test]
    fn chain_proofs_guard_each_hop_and_trace_every_holder() {
        let _lock = crate::gc::global_side_table_test_lock();
        let _no_move = crate::gc::GcSuppressScope::new();
        crate::object::builtin_prototype_value("Object");
        unsafe {
            let holder = crate::object::js_object_alloc(0, 0);
            let middle =
                crate::object::js_object_create(crate::value::js_nanbox_pointer(holder as i64));
            let middle = crate::value::js_nanbox_get_pointer(middle) as *mut ObjectHeader;
            let receiver =
                crate::object::js_object_create(crate::value::js_nanbox_pointer(middle as i64));
            let receiver = crate::value::js_nanbox_get_pointer(receiver) as *mut ObjectHeader;
            let mut proof = ShapeChain::capture(receiver).expect("ordinary chain admitted");
            assert!(proof.valid());
            let mut traced = Vec::new();
            proof.scan(&mut crate::gc::RuntimeRootVisitor::for_copy(&mut |v| {
                traced.push(v.to_bits() & crate::value::POINTER_MASK)
            }));
            assert!(traced.contains(&(holder as u64)));
            assert!(traced.contains(&(middle as u64)));
            let key = crate::string::canonical_key(b"shape_chain_mutation");
            crate::object::js_object_set_field_by_name(middle, key, 3.0);
            assert!(
                !proof.valid(),
                "shadowing a middle hop invalidates its answer"
            );
            let proof = ShapeChain::capture(receiver).unwrap();
            crate::object::prototype_chain::object_set_user_prototype(
                middle as usize,
                crate::value::TAG_NULL,
            );
            assert!(
                !proof.valid(),
                "relinking a middle hop invalidates its answer"
            );
            crate::object::native_this_alias::register_this_to_handle_alias(
                crate::value::js_nanbox_pointer(middle as i64),
                crate::value::js_nanbox_pointer(42),
                false,
            );
            assert!(
                ShapeChain::capture(receiver).is_none(),
                "a native alias used as a prototype can expose virtual properties"
            );
        }
    }
}

impl ShapeChain {
    /// Capture a complete ordinary chain. The caller separately proves the
    /// property's answer; these words validate its continued applicability.
    pub(crate) unsafe fn capture(recv: *const ObjectHeader) -> Option<Self> {
        use crate::object::method_site::read_holder::{admitted_link, class_link, next_from_word};
        use crate::object::shapes::{PROTO_ID_DEFAULT, PROTO_ID_NULL};
        let mut current = recv;
        let mut hops = Vec::new();
        let pid =
            crate::object::shapes::shape_proto_id(crate::object::shapes::object_shape_stamp(recv))?;
        if (crate::object::shapes::PROTO_ID_CLASS..crate::object::shapes::PROTO_ID_MIXED)
            .contains(&pid)
        {
            let slot = crate::object::shapes::identity_word_slot(pid)?;
            let word = *slot;
            if !crate::JSValue::from_bits(word).is_pointer() {
                return None;
            }
            // S7 can replace a CLASS holder without changing the receiver's
            // shape. Consume the identity word before any recorded holder.
            hops.push(ShapeHop {
                addr: slot as usize,
                word,
                heap: false,
            });
        }
        let realm = crate::array::object_prototype_addr_if_resolved();
        for depth in 0..32 {
            if current as usize == realm && depth != 0 {
                break;
            }
            let next = if depth == 0 {
                class_link(current).or_else(|| {
                    let (pid, word) = admitted_link(current)?;
                    if pid == PROTO_ID_NULL {
                        return Some(std::ptr::null());
                    }
                    Some(if pid == PROTO_ID_DEFAULT {
                        realm as *const ObjectHeader
                    } else {
                        next_from_word(current, word)
                    })
                })?
            } else {
                let (pid, word) = admitted_link(current)?;
                if pid == PROTO_ID_NULL {
                    break;
                }
                if pid == PROTO_ID_DEFAULT {
                    realm as *const ObjectHeader
                } else {
                    next_from_word(current, word)
                }
            };
            if next.is_null() {
                return if crate::object::shapes::shape_proto_id(
                    crate::object::shapes::object_shape_stamp(current),
                ) == Some(PROTO_ID_NULL)
                {
                    Some(Self::from_hops(hops))
                } else {
                    None
                };
            }
            if next == current || next == recv || !Self::admitted(next) {
                return None;
            }
            hops.push(ShapeHop {
                addr: next as usize,
                word: std::ptr::read(next as *const u64),
                heap: true,
            });
            current = next;
            if current as usize == realm {
                return Some(Self::from_hops(hops));
            }
        }
        (hops.len() < 32).then(|| Self::from_hops(hops))
    }

    unsafe fn admitted(obj: *const ObjectHeader) -> bool {
        let Some(gc) = crate::value::addr_class::try_read_gc_header(obj as usize) else {
            return false;
        };
        if gc.obj_type != crate::gc::GC_TYPE_OBJECT
            || gc.gc_flags & crate::gc::GC_FLAG_FORWARDED != 0
            || gc._reserved & crate::gc::OBJ_FLAG_TYPED_ARRAY_PROTO != 0
        {
            return false;
        }
        let Some(shape) = crate::object::shapes::object_shape_descriptor(obj) else {
            return false;
        };
        if !shape.object_kind.is_ordinary_layout() || crate::object::dictionary::is_dictionary(obj)
        {
            return false;
        }
        let meta = (*obj).meta;
        meta.is_null()
            || ((*meta).elements == 0
                && (*meta).flags
                    & (crate::object::OBJECT_META_FLAG_EXOTIC_READ_RECEIVER
                        | crate::object::OBJECT_META_FLAG_NATIVE_ALIAS)
                    == 0)
    }

    fn from_hops(hops: Vec<ShapeHop>) -> Self {
        let len = hops.len();
        Self {
            hops: Box::into_raw(hops.into_boxed_slice()) as *mut ShapeHop,
            len,
        }
    }

    #[inline]
    pub(crate) unsafe fn valid(&self) -> bool {
        std::slice::from_raw_parts(self.hops, self.len)
            .iter()
            .all(|hop| std::ptr::read(hop.addr as *const u64) == hop.word)
    }

    pub(crate) fn scan(&mut self, visitor: &mut crate::gc::RuntimeRootVisitor<'_>) {
        for hop in unsafe { std::slice::from_raw_parts_mut(self.hops, self.len) } {
            let addr = &mut hop.addr;
            if hop.heap {
                visitor.visit_tagged_usize_slot(addr, crate::value::POINTER_TAG);
            } else {
                visitor.visit_nanbox_u64_slot(&mut hop.word);
            }
        }
    }
}
