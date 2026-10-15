//! The accessor candidate reuses the one collecting property-read exit.
//! Validation and invocation live in the shared runtime holder consumer.
use super::super::FnCtx;
use crate::types::{I64, I8, PTR};
