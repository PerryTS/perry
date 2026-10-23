//! A native owner can be taken across an async call without borrowing a Store.
//! Other operations see invalid-state unless a control view is exposed. Bind/listen loans
//! restore their owner on cancellation; connection attempts close on failure.
use super::{types::ErrorCode, SocketResult};
use std::sync::{Arc, Mutex};
pub(super) struct Slot<T>(Arc<Mutex<Cell<T>>>);
struct Cell<T> {
    value: Option<T>,
    loaned: bool,
}
impl<T> Clone for Slot<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}
impl<T> Slot<T> {
    pub fn new(value: T) -> Self {
        Self(Arc::new(Mutex::new(Cell {
            value: Some(value),
            loaned: false,
        })))
    }
    pub fn with<R>(&self, f: impl FnOnce(&T) -> crate::p2::SocketResult<R>) -> SocketResult<R> {
        let guard = self.0.lock().unwrap();
        Ok(f(guard.value.as_ref().ok_or(ErrorCode::InvalidState)?)?)
    }
    pub fn with_mut<R>(
        &self,
        f: impl FnOnce(&mut T) -> crate::p2::SocketResult<R>,
    ) -> SocketResult<R> {
        let mut guard = self.0.lock().unwrap();
        Ok(f(guard.value.as_mut().ok_or(ErrorCode::InvalidState)?)?)
    }
    pub fn take(&self, restore: bool) -> SocketResult<Loan<T>> {
        let mut cell = self.0.lock().unwrap();
        if cell.loaned {
            return Err(ErrorCode::InvalidState.into());
        }
        let value = cell.value.take().ok_or(ErrorCode::InvalidState)?;
        cell.loaned = true;
        Ok(Loan {
            slot: self.clone(),
            value: Some(value),
            restore,
        })
    }
}
pub(super) struct Loan<T> {
    slot: Slot<T>,
    value: Option<T>,
    restore: bool,
}
impl<T> Loan<T> {
    /// Permit synchronous controls while preventing a second owner loan.
    pub fn expose(&mut self, control: T) {
        self.slot.0.lock().unwrap().value = Some(control);
    }
    pub fn get(&mut self) -> &mut T {
        self.value.as_mut().unwrap()
    }
    pub fn restore(&mut self) {
        self.restore = true;
    }
}
impl<T> Drop for Loan<T> {
    fn drop(&mut self) {
        let mut cell = self.slot.0.lock().unwrap();
        cell.value = if self.restore {
            self.value.take()
        } else {
            None
        };
        cell.loaned = false;
    }
}
