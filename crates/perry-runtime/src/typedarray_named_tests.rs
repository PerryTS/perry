use super::*;

struct Reset(u8);
impl Drop for Reset {
    fn drop(&mut self) {
        PERRY_TYPED_NAMED_PROPS_INVALIDATED.store(self.0, Ordering::Relaxed);
    }
}
fn pristine() -> Reset {
    Reset(PERRY_TYPED_NAMED_PROPS_INVALIDATED.swap(0, Ordering::Relaxed))
}
fn boxed(addr: usize) -> JSValue {
    JSValue::pointer(addr as *mut u8)
}

#[test]
fn byte_owner_and_offset_view_have_a_positive_metadata_leaf() {
    let _reset = pristine();
    unsafe {
        let b = crate::buffer::buffer_alloc(12);
        (*b).length = 12;
        let view = crate::buffer::js_buffer_slice(b, 3, 9);
        assert_eq!(try_get(boxed(b as usize), b"length"), Some(12.0));
        assert_eq!(try_get(boxed(view as usize), b"length"), Some(6.0));
        assert_eq!(try_get(boxed(view as usize), b"byteLength"), Some(6.0));
        assert_eq!(try_get(boxed(view as usize), b"byteOffset"), Some(3.0));
        assert_eq!(try_get(boxed(b as usize), b"other"), None);
    }
}

#[test]
fn own_metadata_withdraws_the_leaf_before_publication() {
    let _reset = pristine();
    unsafe {
        let b = crate::buffer::buffer_alloc(12);
        (*b).length = 12;
        assert_eq!(try_get(boxed(b as usize), b"length"), Some(12.0));
        crate::buffer::buffer_define_own_data_prop(b as usize, "length", 91.0);
        assert_eq!(try_get(boxed(b as usize), b"length"), None);
        let key = crate::string::intern_ascii_literal(b"length");
        assert_eq!(
            get(f64::from_bits(boxed(b as usize).bits()), key),
            Some(91.0)
        );
    }
}

#[test]
fn an_ordinary_object_never_receives_a_typed_metadata_leaf() {
    let _reset = pristine();
    unsafe {
        let object = crate::object::js_object_alloc(0, 0);
        assert_eq!(try_get(boxed(object as usize), b"length"), None);
        assert_eq!(try_get(JSValue::number(12.0), b"length"), None);
        assert_eq!(try_get(JSValue::null(), b"byteLength"), None);
    }
}
