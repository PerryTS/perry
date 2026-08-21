//! CommonJS-to-ESM source-level transformation for `compilePackages`.
//!
//! Closes the React-class blocker for issue #348 (ink-as-compilePackages).
//!
//! React 18 ships as CommonJS — `node_modules/react/index.js` does
//! `module.exports = require('./cjs/react.production.min.js')`, and the
//! actual implementation file uses `exports.useState = function() {...}`
//! patterns. Perry's native pipeline is ESM-only — `module`/`require` lower
//! to bare-identifier-zero, so the entire react module compiles to a no-op
//! and every downstream `import { useState } from "react"` link-fails with
//! `Undefined symbols: _perry_fn_node_modules_react_index_js__useState`.
//!
//! This module detects CJS at module-read time and rewrites the source to
//! ESM-shaped code before SWC parses it. The wrap pattern (modeled after
//! `perry-jsruntime/src/modules.rs:481` which already does this for the V8
//! fallback) is:
//!
//!   1. Hoist every `require('X')` call as `import _req_N from 'X';`.
//!   2. Wrap the CJS body in an IIFE that defines `module = { exports: {} }`,
//!      a synchronous `require(specifier)` that dispatches to the hoisted
//!      `_req_N` bindings, runs the original code, and returns
//!      `module.exports`. The IIFE result is bound to `_cjs`.
//!   3. Emit `export default _cjs;` plus `export const X = _cjs.X;` for each
//!      detected named export.
//!
//! Two named-export sources are unioned:
//!
//!   - `exports.X = ...` patterns *in this file* (regex; the existing
//!     jsruntime heuristic).
//!   - For "trivial re-export wrappers" (`module.exports = require('./X')`,
//!     optionally inside a `process.env.NODE_ENV` conditional), the
//!     `exports.X = ...` patterns of the recursively-required *target* file.
//!     Without this, react/index.js — whose only meaningful statements are
//!     two conditional `module.exports = require(...)` calls — produces zero
//!     named exports of its own and the link still fails. The recursion
//!     follows up to a small depth (2 levels) to handle one level of env
//!     switching; deeper indirection is rare and gets the no-op fallback.

pub(crate) mod detect;
mod extract_exports;
mod extract_requires;
mod hoist_classes;
mod wrap;

#[cfg(test)]
mod issue_6585_tests;
#[cfg(test)]
mod preamble_canary_tests;

// Cross-sibling helpers — siblings reach for these via `use super::*;`.
use detect::is_js_reserved_word;
use extract_exports::{
    extract_exports_from_source, extract_named_exports_from_require,
    extract_object_literal_exports_from_require, extract_single_module_exports_assignment,
    module_reexport_specs,
};
// #8547: the stdlib-link decision needs the literal `require()` specifiers.
pub(crate) use extract_requires::extract_require_specifiers;
use extract_requires::{
    extract_export_star_specs, extract_require_aliases_with_ranges, function_local_specs,
    identifier_is_declared_binding, identifier_is_reassigned,
};
use hoist_classes::{
    extract_top_level_class_decls, rewrite_module_exports_class_expression,
    source_has_top_level_return, top_level_class_names,
};

// Public API consumed by `compile.rs` / `collect_modules.rs`.
pub(super) use detect::is_commonjs;
pub(super) use wrap::{wrap_commonjs_for_target, wrap_commonjs_with_body_offset};

#[cfg(test)]
mod tests;
