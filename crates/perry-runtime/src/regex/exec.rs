//! Public RegExpBuiltinExec entry point using the shared Perex host path.
use super::*;

#[no_mangle]
pub extern "C" fn js_regexp_exec(
    re: *mut RegExpHeader,
    s: *const StringHeader,
) -> *mut crate::array::ArrayHeader {
    if !is_valid_regex_ptr(re) || !is_valid_ptr(s) {
        LAST_EXEC_INDEX.with(|idx| *idx.borrow_mut() = -1.0);
        LAST_EXEC_GROUPS.with(|g| *g.borrow_mut() = ptr::null_mut());
        return ptr::null_mut();
    }
    if crate::hot_diag::regex_on() {
        diag_note_op(re, crate::hot_diag::RegexOp::Exec);
    }
    let found = perex_api::finish(perex_api::execute(re, s, true, &mut perex_runtime::poll));
    match found {
        Some(found) => {
            LAST_EXEC_INDEX.with(|idx| *idx.borrow_mut() = found.full.start() as f64);
            LAST_EXEC_GROUPS.with(|g| *g.borrow_mut() = found.groups);
            found.array
        }
        None => {
            LAST_EXEC_INDEX.with(|idx| *idx.borrow_mut() = -1.0);
            LAST_EXEC_GROUPS.with(|g| *g.borrow_mut() = ptr::null_mut());
            ptr::null_mut()
        }
    }
}
