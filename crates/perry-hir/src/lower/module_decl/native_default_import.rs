//! Native/CJS-style default-import classification helpers — extracted from
//! `lower/module_decl.rs` (pure mechanical split, no logic changes).

#![allow(unused_imports)]

use anyhow::{anyhow, Result};
use perry_types::{FuncId, FunctionType, GlobalId, LocalId, Type, TypeParam};
use std::collections::{HashMap, HashSet};
use swc_ecma_ast as ast;

use super::*;
use crate::ir::*;

pub(crate) fn is_cjs_style_native_default_import(module_name: &str) -> bool {
    matches!(
        module_name,
        "async_hooks"
            | "child_process"
            | "cluster"
            | "constants"
            | "dns"
            | "dns/promises"
            | "events"
            | "inspector"
            | "inspector/promises"
            | "module"
            | "os"
            | "path"
            | "path/posix"
            | "path/win32"
            | "punycode"
            | "querystring"
            | "sys"
            | "url"
            | "util"
    )
}

pub(crate) fn node_submodule_default_export_key(module_name: &str) -> Option<&'static str> {
    match module_name {
        "test/reporters" => Some("test_reporters"),
        _ => None,
    }
}
