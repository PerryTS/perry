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
        task.job_context_mut().prepare_job_enqueue();
        self.push_back(task);
    }
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
