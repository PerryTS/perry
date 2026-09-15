//! O(1) per-agent timer liveness. Membership follows the queue record's lifetime.
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, LazyLock, Mutex, Weak};

#[derive(Default)]
struct Counts([AtomicUsize; 6]);
static MEMBERS: LazyLock<Mutex<HashMap<i64, Weak<Member>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

// Keep primary counts available across UI pump threads. Worker counts vanish
// after their last timer and TLS cache are released.
static PRIMARY: LazyLock<Arc<Counts>> = LazyLock::new(|| Arc::new(Counts::default()));
thread_local! {
    static LOCAL: std::cell::RefCell<Option<(crate::agent::AgentId, Arc<Counts>)>> =
        const { std::cell::RefCell::new(None) };
}

fn counts() -> Arc<Counts> {
    let owner = crate::agent::current_agent();
    if owner == crate::agent::PRIMARY_AGENT {
        return PRIMARY.clone();
    }
    LOCAL.with(|slot| {
        let mut slot = slot.borrow_mut();
        if let Some((id, counts)) = slot.as_ref() {
            if *id == owner {
                return counts.clone();
            }
        }
        // Worker IDs are minted on this thread and cannot be adopted by a
        // different thread. Tokens retain these counts until agent retirement.
        let counts = Arc::new(Counts::default());
        *slot = Some((owner, counts.clone()));
        counts
    })
}

struct Member {
    counts: Arc<Counts>,
    kind: usize,
    active: AtomicBool,
    retired: AtomicBool,
}

impl Member {
    fn set_ref(&self, active: bool) {
        if active && self.retired.load(Ordering::Acquire) {
            return;
        }
        let previous = self.active.swap(active, Ordering::AcqRel);
        if previous == active {
            return;
        }
        if active {
            self.counts.0[self.kind].fetch_add(1, Ordering::AcqRel);
        } else {
            let previous = self.counts.0[self.kind].fetch_sub(1, Ordering::AcqRel);
            debug_assert!(previous > 0, "timer liveness underflow");
        }
    }
}

impl Drop for Member {
    fn drop(&mut self) {
        self.set_ref(false);
        let previous = self.counts.0[self.kind + 3].fetch_sub(1, Ordering::AcqRel);
        debug_assert!(previous > 0, "timer membership underflow");
    }
}

pub(super) struct Membership {
    member: Arc<Member>,
    id: Option<i64>,
}

impl Membership {
    pub(super) fn new(kind: usize, id: Option<i64>, active: bool) -> Self {
        let member = Arc::new(Member {
            counts: counts(),
            kind,
            active: AtomicBool::new(false),
            retired: AtomicBool::new(false),
        });
        member.counts.0[kind + 3].fetch_add(1, Ordering::AcqRel);
        member.set_ref(active);
        if let Some(id) = id {
            MEMBERS.lock().unwrap().insert(id, Arc::downgrade(&member));
        }
        Self { member, id }
    }

    /// A detached expired batch no longer contributes, even while its callbacks
    /// are running (which may themselves enter a nested await loop).
    pub(super) fn retire(&self) {
        self.member.retired.store(true, Ordering::Release);
        self.member.set_ref(false);
    }
}

impl Drop for Membership {
    fn drop(&mut self) {
        if let Some(id) = self.id {
            let mut members = MEMBERS.lock().unwrap();
            if members
                .get(&id)
                .is_some_and(|member| member.ptr_eq(&Arc::downgrade(&self.member)))
            {
                members.remove(&id);
            }
        }
    }
}

pub(super) fn set_ref(id: i64, active: bool) {
    let member = MEMBERS.lock().unwrap().get(&id).and_then(Weak::upgrade);
    if let Some(member) = member {
        member.set_ref(active);
    }
}

pub(super) fn has_any(kind: usize) -> bool {
    counts().0[kind + 3].load(Ordering::Acquire) != 0
}

pub(super) fn has_refed(kind: usize) -> bool {
    counts().0[kind].load(Ordering::Acquire) != 0
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn balance_open_close_unref_error_cancel_and_agent_retirement() {
        let baseline = counts().0[0].load(Ordering::Acquire);
        let timer = Membership::new(0, Some(-987654), true);
        assert_eq!(counts().0[0].load(Ordering::Acquire), baseline + 1);
        set_ref(-987654, false);
        set_ref(-987654, false);
        assert_eq!(counts().0[0].load(Ordering::Acquire), baseline);
        set_ref(-987654, true);
        drop(timer);
        assert_eq!(counts().0[0].load(Ordering::Acquire), baseline);
        let failed: Result<(), ()> = (|| {
            let _timer = Membership::new(0, None, true);
            assert_eq!(counts().0[0].load(Ordering::Acquire), baseline + 1);
            Err(())
        })();
        assert!(failed.is_err());
        let mut cancelled = vec![Membership::new(0, None, true)];
        cancelled.clear();
        assert_eq!(counts().0[0].load(Ordering::Acquire), baseline);
        std::thread::spawn(|| {
            let agent = crate::agent::enter_worker_agent();
            assert!(!has_refed(0));
            let timer = Membership::new(0, None, true);
            assert!(has_refed(0));
            drop(timer);
            assert!(!has_refed(0));
            crate::agent::retire_agent(agent);
        })
        .join()
        .unwrap();
        assert_eq!(counts().0[0].load(Ordering::Acquire), baseline);
    }
}
