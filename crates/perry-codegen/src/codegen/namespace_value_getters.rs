//! Closure-ABI adapters for live exports in materialized static namespaces.

use super::helpers::sanitize_member;
use super::opts::CrossModuleCtx;
use crate::module::LlModule;
use crate::types::{DOUBLE, I64};

pub(crate) fn symbol(module_prefix: &str, namespace: &str, member: &str) -> String {
    format!(
        "__perry_namespace_value_{}__{}",
        module_prefix,
        sanitize_member(&format!("{namespace}:{member}"))
    )
}

pub(super) fn emit(llmod: &mut LlModule, module_prefix: &str, imports: &CrossModuleCtx) {
    let mut members: Vec<_> = imports.namespace_member_prefixes.iter().collect();
    members.sort_by_key(|(key, _)| *key);
    for ((namespace, member), source_prefix) in members {
        let wrapper_name = symbol(module_prefix, namespace, member);
        // Lowering declares an adapter only when a namespace value actually
        // escapes. Direct `ns.x` reads need no additional wrapper.
        if !llmod.is_declared(&wrapper_name) || llmod.has_function(&wrapper_name) {
            continue;
        }
        let origin = crate::expr::import_origin_suffix_ns(
            &imports.import_function_origin_names,
            &imports.namespace_member_origin_names,
            namespace,
            member,
        );
        let getter_name = format!("perry_fn_{source_prefix}__{origin}");
        llmod.declare_function(&getter_name, DOUBLE, &[]);
        let wrapper =
            llmod.define_function(wrapper_name, DOUBLE, vec![(I64, "%closure".to_string())]);
        wrapper.create_block("entry");
        let block = wrapper.block_mut(0).unwrap();
        let value = block.call(DOUBLE, &getter_name, &[]);
        block.ret(DOUBLE, &value);
    }
}
