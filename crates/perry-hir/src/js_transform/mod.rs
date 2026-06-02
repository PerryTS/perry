//! JavaScript import transformation
//!
//! This module transforms imports from JavaScript modules into V8 runtime calls.
//! When an import comes from a JS module (ModuleKind::Interpreted), this pass:
//! 1. Creates a module handle variable for each JS module
//! 2. Adds initialization code to load the module via JsLoadModule
//! 3. Transforms function calls to imported functions into JsCallFunction calls
//! 4. Transforms method calls on JS objects to JsCallMethod
//! 5. Transforms property access on JS objects to JsGetProperty/JsSetProperty
//! 6. Transforms new expressions for JS classes to JsNew
//! 7. Wraps closures passed to JS functions with JsCreateCallback

use crate::ir::{Expr, Module, ModuleKind, Stmt};
use perry_types::LocalId;
use std::collections::{BTreeMap, HashMap, HashSet};

mod cross_module_natives;
mod imports;
mod local_natives;

pub use cross_module_natives::{fix_cross_module_native_instances, ExportedNativeInstance};
pub use imports::{transform_js_imports, JsImportInfo};
pub use local_natives::fix_local_native_instances;
