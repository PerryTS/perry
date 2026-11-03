/// True when `class_id` or an ancestor extends the intrinsic Promise.
pub(crate) fn promise_parent_in_chain(class_id: u32) -> bool {
    let mut cid = class_id;
    let mut depth = 0u32;
    while depth < 32 && cid != 0 {
        let parent = js_get_dynamic_parent_value(cid);
        if matches!(
            super::super::global_this_tables::constructor_identity(parent),
            Some(BuiltinConstructorIdentity::Promise)
        ) {
            return true;
        }
        match get_parent_class_id(cid) {
            Some(parent_id) if parent_id != 0 && parent_id != cid => {
                cid = parent_id;
                depth += 1;
            }
            _ => break,
        }
    }
    false
}
