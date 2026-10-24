//! One driver owned by a standalone embedding thread, shared by every store.
//! Perry executables inject the runtime's operation table instead.
use super::*;
use std::cell::RefCell;
use turnloop::{Completions, Config};
pub struct Standalone;
struct Driver {
    driver: Loop,
    completions: Completions,
    staged: Vec<Completion>,
}
thread_local! {
    static DRIVER: RefCell<Option<Driver>> = const { RefCell::new(None) };
}
fn with<R>(f: impl FnOnce(&mut Driver) -> turnloop::Result<R>) -> turnloop::Result<R> {
    DRIVER.with(|slot| {
        let mut slot = slot.try_borrow_mut().map_err(|_| invalid())?;
        if slot.is_none() {
            *slot = Some(Driver {
                driver: Loop::new(Config::default())?,
                completions: Completions::default(),
                staged: Vec::new(),
            });
        }
        f(slot.as_mut().expect("initialized embedding driver"))
    })
}
impl Embedding for Standalone {
    fn with_driver<R>(f: impl FnOnce(&mut Loop) -> turnloop::Result<R>) -> turnloop::Result<R> {
        with(|state| f(&mut state.driver))
    }
    fn turn_and_take(
        operations: &[OpId],
        closing: &[Handle],
        timeout: Timeout,
    ) -> turnloop::Result<Vec<Completion>> {
        with(|state| {
            fn take(
                staged: &mut Vec<Completion>,
                operations: &[OpId],
                closing: &[Handle],
            ) -> Vec<Completion> {
                staged
                    .extract_if(.., |c| {
                        c.token == TOKEN
                            && (c.op.is_some_and(|id| operations.contains(&id))
                                || (matches!(c.result, OpResult::Closed)
                                    && c.handle.is_some_and(|h| closing.contains(&h))))
                    })
                    .collect()
            }
            let complete = take(&mut state.staged, operations, closing);
            if !complete.is_empty() {
                return Ok(complete);
            }
            state.driver.turn(timeout, &mut state.completions)?;
            state.staged.extend(state.completions.drain());
            Ok(take(&mut state.staged, operations, closing))
        })
    }
}
pub fn ops() -> &'static abi::Ops {
    &Provider::<Standalone>::OPS
}
