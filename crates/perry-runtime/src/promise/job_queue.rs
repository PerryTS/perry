//! One enqueue boundary for ownership and wake notification. The queue remains
//! the existing scanned VecDeque; native domain tokens hold no JS pointers.
use super::*;

impl Task {
    fn job_context(&self) -> &AsyncContextSnapshot {
        match self {
            Task::Promise(_, _, _, context)
            | Task::PromiseAll(_, _, _, context)
            | Task::Inline(_, _, _, _, context)
            | Task::AsyncStep(_, _, _, _, context, _, _, _)
            | Task::Microtask { context, .. } => context,
        }
    }

    fn job_context_mut(&mut self) -> &mut AsyncContextSnapshot {
        match self {
            Task::Promise(_, _, _, context)
            | Task::PromiseAll(_, _, _, context)
            | Task::Inline(_, _, _, _, context)
            | Task::AsyncStep(_, _, _, _, context, _, _, _)
            | Task::Microtask { context, .. } => context,
        }
    }
}

pub(super) trait EnqueuePromiseJob {
    fn enqueue_promise_job(&mut self, task: Task);
}

impl EnqueuePromiseJob for std::collections::VecDeque<Task> {
    fn enqueue_promise_job(&mut self, mut task: Task) {
        if task.job_context_mut().prepare_job_enqueue() {
            self.push_back(task);
        }
    }
}

/// Retire roots without invoking cancelled callbacks. Late producers reject
/// the same tokens at enqueue, so dropping the task cannot resurrect its jobs.
pub(crate) fn discard_cancelled_guest_jobs() {
    TASK_QUEUE.with(|q| {
        q.borrow_mut()
            .retain(|job| !job.job_context().job_is_cancelled())
    });
    crate::builtins::discard_cancelled_microtasks();
    crate::async_hooks::discard_cancelled_destroy_notifications();
    crate::node_submodules::diagnostics::discard_cancelled_uncaught();
    crate::weakref::discard_cancelled_finalization_jobs();
}

pub(super) fn pop_task() -> Option<Task> {
    TASK_QUEUE.with(|queue| {
        let mut queue = queue.borrow_mut();
        let index = queue
            .iter()
            .position(|task| task.job_context().job_is_eligible())?;
        queue.remove(index)
    })
}

pub(super) fn eligible_task_pending() -> bool {
    TASK_QUEUE.with(|queue| {
        queue
            .borrow()
            .iter()
            .any(|task| task.job_context().job_is_eligible())
    })
}

/// Native-only readiness query, including jobs queued outside any task. A C
/// scheduler must wake an entering task for those jobs too, even without a hint.
pub(crate) fn eligible_guest_jobs_pending() -> bool {
    eligible_task_pending()
        || crate::builtins::queued_microtasks_pending()
        || crate::async_hooks::gc_destroy_work_pending()
        || crate::weakref::eligible_finalization_jobs_pending()
        || crate::node_submodules::diagnostics::uncaught_work_pending()
}
