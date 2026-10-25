//! Canonical future/stream transport helpers share private, quota-limited memory.
#[cfg(test)]
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

#[cfg(test)]
#[path = "async_values/reader.rs"]
mod reader;
#[cfg(test)]
#[path = "async_values/reader_cleanup.rs"]
mod reader_cleanup;
#[cfg(test)]
#[path = "async_values/reader_tests.rs"]
mod reader_tests;
#[cfg(test)]
#[path = "async_values/reader_types.rs"]
mod reader_types;
