//! Public RegExpBuiltinExec entry point using the shared Perex host path.
use super::*;

#[no_mangle]
pub extern "C" fn js_regexp_exec(
    re: *mut RegExpHeader,
    s: *const StringHeader,
) -> *mut crate::array::ArrayHeader {
    if !instance::is_branded(crate::value::js_nanbox_pointer((re) as i64)) || !is_valid_ptr(s) {
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

/// Internal result-use projection. `method` is Get(receiver, "exec") evaluated
/// BEFORE `argument`, exactly as for the source method call. A failed brand or
/// builtin proof invokes that same method; there is no second property Get.
#[no_mangle]
pub extern "C" fn js_regexp_exec_projected(
    receiver: f64,
    method: f64,
    argument: f64,
    capture: f64,
) -> f64 {
    let scope = crate::gc::RuntimeHandleScope::new();
    let receiver = scope.root_nanbox_f64(receiver);
    let method = scope.root_nanbox_f64(method);
    let argument = scope.root_nanbox_f64(argument);
    let branded = instance::is_branded(receiver.get_nanbox_f64());
    if !branded
        || !crate::object::regex_proto_thunks::is_builtin_regexp_exec(method.get_nanbox_f64())
    {
        return perex_api::finish(perex_dispatch::call_one(&method, &receiver, &argument));
    }
    // Coercion can invoke arbitrary code (including compile/lastIndex writes).
    // The same builtin thunk does it before executing, so root the receiver
    // above and only inspect its current data in execute_rooted afterwards.
    let input = perex_api::finish(perex_dispatch::to_string(&argument));
    let input = scope.root_string_ptr(input);
    let found = perex_api::finish(perex_api::execute_capture_rooted(
        &receiver,
        &input,
        capture as usize,
        &mut perex::Budget::new(perex_api::WORK),
        &perex_memory::MemoryBudget::new(perex_api::SCRATCH_BYTES),
        &mut perex_runtime::poll,
        None,
    ));
    found.map_or(f64::from_bits(crate::value::TAG_NULL), |found| {
        crate::value::js_nanbox_pointer(found.array as i64)
    })
}
