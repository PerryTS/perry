use super::{Detached, invalid, process::Child, signals::Subscription, unsupported};
use crate::slots::Slots;
use crate::{
    backend::{Event, Operation, Outcome, Request},
    *,
};
enum Service {
    Child(Child),
    Signal(Signal, Subscription),
}
struct Entry {
    handle: Handle,
    service: Service,
    op: Option<OpId>,
    cancelled: bool,
    closing: bool,
}
pub(super) struct Services {
    entries: Slots<Entry>,
    pending: usize,
}
impl Services {
    pub(super) fn new(capacity: usize) -> Self {
        Self {
            entries: Slots::new(capacity),
            pending: 0,
        }
    }
    pub(super) fn contains(&self, handle: Handle) -> bool {
        self.entries
            .get(handle.index())
            .and_then(Option::as_ref)
            .is_some_and(|e| e.handle == handle)
    }
    pub(super) fn child(&mut self, handle: Handle, child: Child) {
        self.entries[handle.index()] = Some(Entry {
            handle,
            service: Service::Child(child),
            op: None,
            cancelled: false,
            closing: false,
        });
    }
    pub(super) fn signal(
        &mut self,
        handle: Handle,
        signal: Signal,
        notifier: Notifier,
    ) -> Result<()> {
        let subscription = Subscription::new(signal, notifier)?;
        self.entries[handle.index()] = Some(Entry {
            handle,
            service: Service::Signal(signal, subscription),
            op: None,
            cancelled: false,
            closing: false,
        });
        Ok(())
    }
    pub(super) fn submit(&mut self, request: Request) -> Result<()> {
        let entry = self.entries[request.handle.index()]
            .as_mut()
            .ok_or_else(invalid)?;
        if entry.handle != request.handle || entry.op.is_some() {
            return Err(invalid());
        }
        if !matches!(
            (&entry.service, request.operation),
            (Service::Child(_), Operation::ProcessExit)
                | (Service::Signal(..), Operation::WatchSignal)
        ) {
            return Err(unsupported());
        }
        entry.op = Some(request.op);
        self.pending += 1;
        entry.cancelled = false;
        Ok(())
    }
    pub(super) fn cancel(&mut self, op: OpId) -> bool {
        if let Some(entry) = self.entries.iter_mut().flatten().find(|e| e.op == Some(op)) {
            // Close retains the exit watch until the child has terminated. A
            // standalone watch cancellation needs no process teardown or reap.
            entry.cancelled = !entry.closing;
            true
        } else {
            false
        }
    }
    pub(super) fn close(&mut self, handle: Handle) -> Result<()> {
        if let Some(entry) = self
            .entries
            .get_mut(handle.index())
            .and_then(Option::as_mut)
            && entry.handle == handle
            && let Service::Child(child) = &mut entry.service
        {
            child.close()?;
            entry.closing = true;
            entry.cancelled = false;
        }
        Ok(())
    }
    pub(super) fn kill(&mut self, handle: Handle, signal: Signal, group: bool) -> Result<()> {
        let entry = self
            .entries
            .get_mut(handle.index())
            .and_then(Option::as_mut)
            .filter(|e| e.handle == handle)
            .ok_or(Error::new(ErrorKind::NotFound))?;
        match &mut entry.service {
            Service::Child(child) => child.kill(signal, group),
            _ => Err(unsupported()),
        }
    }
    pub(super) fn release(&mut self, handle: Handle) {
        if self.contains(handle) {
            self.entries[handle.index()] = None;
        }
    }
    pub(super) fn has_work(&self) -> bool {
        if self.pending == 0 {
            return false;
        }
        self.entries.iter().flatten().any(|e| {
            e.op.is_some()
                && (e.cancelled
                    || match &e.service {
                        Service::Child(child) => child.ready(),
                        Service::Signal(_, signal) => signal.ready(),
                    })
        })
    }
    pub(super) fn collect(&mut self, out: &mut Vec<Event<Detached>>) -> Result<()> {
        if self.pending == 0 {
            return Ok(());
        }
        for entry in self.entries.iter_mut().flatten() {
            if out.len() == out.capacity() {
                break;
            }
            let Some(op) = entry.op else {
                continue;
            };
            let result = if entry.cancelled {
                Some((Outcome::Cancelled, true))
            } else {
                match &mut entry.service {
                    Service::Child(child) => child.status()?.map(|status| {
                        (
                            if entry.closing {
                                Outcome::Cancelled
                            } else {
                                Outcome::Exited(status)
                            },
                            true,
                        )
                    }),
                    Service::Signal(signal, ticket) => {
                        if ticket.take() {
                            Some((Outcome::Signal(*signal), false))
                        } else {
                            None
                        }
                    }
                }
            };
            if let Some((outcome, terminal)) = result {
                if terminal {
                    entry.op = None;
                    self.pending -= 1;
                }
                out.push(Event {
                    op,
                    terminal,
                    result: Ok(outcome),
                });
            }
        }
        Ok(())
    }
}
