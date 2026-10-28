//! Canonical guest task ownership, separate from user-restorable ALS values.
//! Tokens contain native flags only; the snapshot's existing slots own all JS.
use super::*;
use std::{
    marker::PhantomData,
    rc::Rc,
    sync::atomic::{AtomicBool, AtomicU8},
};

const LIVE: u8 = 0;
const EXITED: u8 = 1;
const DOMAIN_CANCELLED: u8 = 2;

struct State {
    status: AtomicU8,
    ready: AtomicBool,
}

/// Opaque native metadata carried by snapshots and deferred notifications.
#[derive(Clone)]
pub struct JobDomainToken(Arc<State>);

impl JobDomainToken {
    pub(crate) fn is_current_live(&self) -> bool {
        self.0.status.load(Ordering::Acquire) == LIVE
            && current().is_some_and(|active| Arc::ptr_eq(&self.0, &active.0))
    }
}

/// One canonical task owns this lease. Snapshots can outlive its EXIT without
/// retaining a task or a native registry entry. The lease stays on its agent.
pub(crate) struct JobDomain {
    token: JobDomainToken,
    _agent: PhantomData<Rc<()>>,
}

impl JobDomain {
    pub(crate) fn new() -> Self {
        Self {
            token: JobDomainToken(Arc::new(State {
                status: AtomicU8::new(LIVE),
                ready: AtomicBool::new(false),
            })),
            _agent: PhantomData,
        }
    }

    /// All Rust scopes must end before the caller returns canonical WAIT/YIELD.
    /// The existing landing-pad guard restores identity even on a JS throw.
    #[cfg(test)]
    pub(crate) fn run<T>(&self, body: impl FnOnce() -> T) -> T {
        run(self.token(), body)
    }

    pub(crate) fn token(&self) -> JobDomainToken {
        self.token.clone()
    }

    pub(crate) fn take_ready(&self) -> bool {
        self.token.0.ready.swap(false, Ordering::AcqRel)
    }

    pub(crate) fn cancel(&self) {
        self.token
            .0
            .status
            .store(DOMAIN_CANCELLED, Ordering::Release);
        self.token.0.ready.store(false, Ordering::Release);
    }

    pub(crate) fn is_cancelled(&self) -> bool {
        cancelled(&Some(self.token()))
    }

    #[cfg(test)]
    pub(crate) fn is_current(&self) -> bool {
        current().is_some_and(|token| Arc::ptr_eq(&token.0, &self.token.0))
    }
}

impl Drop for JobDomain {
    fn drop(&mut self) {
        // EXIT permits adoption. Cancellation is terminal even after the
        // affine lease disappears and only reaction snapshots retain a token.
        let _ =
            self.token
                .0
                .status
                .compare_exchange(LIVE, EXITED, Ordering::AcqRel, Ordering::Acquire);
    }
}

pub(crate) fn current() -> Option<JobDomainToken> {
    ACTIVE_CONTEXT.with(|ctx| ctx.borrow().job_domain.clone())
}

#[inline(never)]
pub(crate) fn run<T>(token: JobDomainToken, body: impl FnOnce() -> T) -> T {
    let previous = replace_current(Some(token));
    push_context_guard(ContextGuardAction::RestoreJobDomain(previous));
    let value = body();
    if let Some(action) = pop_context_guard() {
        apply_context_guard(action);
    }
    value
}

pub(super) fn replace_current(domain: Option<JobDomainToken>) -> Option<JobDomainToken> {
    ACTIVE_CONTEXT.with(|ctx| std::mem::replace(&mut ctx.borrow_mut().job_domain, domain))
}

/// Unowned jobs and jobs left after EXIT can run on the next entering task.
/// Jobs of another live canonical task remain parked, in their original order.
pub(crate) fn eligible(domain: &Option<JobDomainToken>) -> bool {
    domain
        .as_ref()
        .is_none_or(|owner| match owner.0.status.load(Ordering::Acquire) {
            EXITED => true,
            LIVE => current().is_some_and(|active| Arc::ptr_eq(&owner.0, &active.0)),
            _ => false,
        })
}

pub(crate) fn cancelled(domain: &Option<JobDomainToken>) -> bool {
    domain
        .as_ref()
        .is_some_and(|owner| owner.0.status.load(Ordering::Acquire) == DOMAIN_CANCELLED)
}

/// Cancelled reactions must retain their owner identity, including when a
/// sibling settles their Promise after the original task frame was dropped.
pub(crate) fn prepare_enqueue(domain: &mut Option<JobDomainToken>) -> bool {
    if cancelled(domain) {
        return false;
    }
    if domain
        .as_ref()
        .is_none_or(|owner| owner.0.status.load(Ordering::Acquire) == EXITED)
    {
        *domain = current();
    }
    if cancelled(domain) {
        return false;
    }
    if let Some(owner) = domain {
        owner.0.ready.store(true, Ordering::Release);
    }
    true
}

impl AsyncContextSnapshot {
    pub(crate) fn job_is_eligible(&self) -> bool {
        eligible(&self.job_domain)
    }

    pub(crate) fn job_is_cancelled(&self) -> bool {
        cancelled(&self.job_domain)
    }

    pub(crate) fn prepare_job_enqueue(&mut self) -> bool {
        prepare_enqueue(&mut self.job_domain)
    }

    pub(crate) fn job_domain_token(&self) -> Option<JobDomainToken> {
        self.job_domain.clone()
    }
}

#[cfg(test)]
mod tests;
