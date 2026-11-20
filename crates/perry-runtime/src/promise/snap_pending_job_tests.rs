//! SNAPK3c: the census names each queued job by its kind, its callback's
//! code address and the init that queued it (`snap_pending_job_list`).
use super::*;

extern "C" fn job_a(
    _closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
    _v: f64,
) -> f64 {
    1.0
}

extern "C" fn job_b(
    _closure: *const crate::closure::ClosureHeader,
    _this: crate::closure::JsThis,
    _v: f64,
) -> f64 {
    2.0
}

#[test]
fn pending_jobs_name_kind_callback_and_queueing_init() {
    let info_a = crate::fn_info!(job_a, 1);
    let info_b = crate::fn_info!(job_b, 1);
    let code_a = unsafe { (*info_a).code as usize };
    let code_b = unsafe { (*info_b).code as usize };
    assert_ne!(code_a, code_b);
    let cb_a = crate::closure::js_closure_alloc(info_a, 0) as ClosurePtr;
    let cb_b = crate::closure::js_closure_alloc(info_b, 0) as ClosurePtr;
    let undefined = f64::from_bits(crate::value::TAG_UNDEFINED);
    let mut first = capture_context();
    first.snap_init = 7;
    let mut second = capture_context();
    second.snap_init = 11;
    let mut third = capture_context();
    third.snap_init = 12;
    TASK_QUEUE.with(|q| {
        let mut q = q.borrow_mut();
        q.clear();
        q.push_back(Task::Inline(cb_a, undefined, ptr::null_mut(), true, first));
        q.push_back(Task::Microtask {
            callback: cb_b,
            context: second,
            async_id: 0,
            trigger_async_id: 0,
            resource: undefined,
        });
        q.push_back(Task::Inline(
            ptr::null(),
            undefined,
            ptr::null_mut(),
            true,
            third,
        ));
    });
    let jobs = snap_pending_job_list().expect("queue not borrowed");
    TASK_QUEUE.with(|q| q.borrow_mut().clear());
    assert_eq!(
        jobs,
        vec![
            SnapPendingJob {
                kind: "inline",
                code: code_a,
                owner: 7
            },
            SnapPendingJob {
                kind: "microtask",
                code: code_b,
                owner: 11
            },
            SnapPendingJob {
                kind: "inline",
                code: 0,
                owner: 12
            },
        ]
    );
}
