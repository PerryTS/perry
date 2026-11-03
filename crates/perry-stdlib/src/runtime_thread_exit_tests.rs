//! Thread-exit fixtures for runtime and stdlib state that still has external owners.

// #11471: one file per audited group of process-global tables.
mod listeners_tests;
mod reactors_tests;
mod stdlib_misc_tests;
mod streams_tests;
mod symbols_tests;
mod ui_tests;
