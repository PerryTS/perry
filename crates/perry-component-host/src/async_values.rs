//! Canonical future/stream transport helpers share private, quota-limited memory.
const HEAP: &str = include_str!("async_values/heap.wat");

#[cfg(test)]
#[path = "async_values/heap_tests.rs"]
mod heap_tests;

#[cfg(test)]
#[path = "async_values/task_context_tests.rs"]
mod task_context_tests;

#[cfg(test)]
#[path = "async_values/contract_tests.rs"]
mod contract_tests;

#[cfg(test)]
#[path = "async_values/cancel_tests.rs"]
mod cancel_tests;

#[path = "async_values/reader.rs"]
mod reader;
#[path = "async_values/reader_cleanup.rs"]
mod reader_cleanup;
#[cfg(test)]
#[path = "async_values/reader_tests.rs"]
mod reader_tests;
#[path = "async_values/reader_types.rs"]
mod reader_types;
#[path = "async_values/writer.rs"]
mod writer;

#[path = "async_values/reader_dispose.rs"]
mod reader_dispose;

#[path = "async_values/compose.rs"]
mod compose;
pub(crate) use compose::Prepared;
#[path = "async_values/control.rs"]
mod control;
#[path = "async_values/values.rs"]
pub(crate) mod values;
pub(crate) use values::{Adapter, Begin, Values};

#[cfg(test)]
#[path = "async_values/integration_tests.rs"]
mod integration_tests;

#[cfg(test)]
#[path = "async_values/integration_cancel_tests.rs"]
mod integration_cancel_tests;

#[cfg(test)]
#[path = "async_values/transfer_tests.rs"]
mod transfer_tests;

#[cfg(test)]
#[path = "async_values/writer_tests.rs"]
mod writer_tests;

#[path = "async_values/writers.rs"]
pub(crate) mod writers;
pub use writers::AsyncValueType;

#[cfg(test)]
#[path = "async_values/producer_tests.rs"]
mod producer_tests;
