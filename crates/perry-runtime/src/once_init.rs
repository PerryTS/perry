//! `OnceLock::get_or_init` through a plain `fn() -> T` (binary size).
//!
//! `get_or_init` is generic over the init closure, so every call site with its
//! own closure instantiates `OnceLock<T>::initialize` again — ~130 copies in
//! the runtime, most of them `OnceLock<bool>` caches of one env knob each.
//! A non-capturing closure coerces to `fn() -> T`, so routing the call through
//! this helper leaves exactly one instantiation per `T`.

use std::sync::OnceLock;

#[inline]
pub(crate) fn get_or_init<T>(cell: &OnceLock<T>, init: fn() -> T) -> &T {
    cell.get_or_init(init)
}
