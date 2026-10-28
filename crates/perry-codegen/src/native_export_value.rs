//! A static member names its export declaration, not its containing dispatcher.
use crate::{
    block::LlBlock,
    strings::StringPool,
    types::{DOUBLE, I32, PTR},
};

pub(crate) enum PreparedExport {
    Callable {
        symbol: String,
        observe: &'static str,
    },
    SubmoduleMember {
        install: &'static str,
        source: String,
        member: String,
        helper: &'static str,
    },
    Submodule {
        install: &'static str,
        key: String,
        key_len: String,
        default: String,
    },
}
impl PreparedExport {
    pub(crate) fn emit(self, block: &mut LlBlock) -> String {
        match self {
            Self::Callable { symbol, observe } => block.call(DOUBLE, &symbol, &[(I32, observe)]),
            Self::SubmoduleMember {
                install,
                source,
                member,
                helper,
            } => {
                block.call_void(install, &[]);
                let source = block.load(DOUBLE, &source);
                let member = block.load(DOUBLE, &member);
                block.call(DOUBLE, helper, &[(DOUBLE, &source), (DOUBLE, &member)])
            }
            Self::Submodule {
                install,
                key,
                key_len,
                default,
            } => {
                block.call_void(install, &[]);
                block.call(
                    DOUBLE,
                    "js_node_submodule_export_as_function",
                    &[(PTR, &key), (I32, &key_len), (PTR, &default), (I32, "7")],
                )
            }
        }
    }
}
pub(crate) fn prepare(
    strings: &mut StringPool,
    source: &str,
    export: &str,
    observe_writes: bool,
) -> Option<PreparedExport> {
    if source.strip_prefix("node:").unwrap_or(source) == "fs/promises" && export == "constants" {
        return prepare(strings, "fs", "constants", observe_writes);
    }
    if export != "default" {
        if let Some(symbol) =
            perry_dispatch::native_submodule_exports::export_value_symbol(source, export)
        {
            return Some(PreparedExport::Callable {
                symbol: symbol.to_string(),
                observe: if observe_writes { "1" } else { "0" },
            });
        }
    }
    let callable_source = match source.strip_prefix("node:").unwrap_or(source) {
        "util/types" => "util.types",
        "path/posix" => "path.posix",
        "path/win32" => "path.win32",
        other => other,
    };
    if let Some(symbol) =
        perry_dispatch::native_callable_exports::export_value_symbol(callable_source, export)
    {
        return Some(PreparedExport::Callable {
            symbol,
            observe: if observe_writes { "1" } else { "0" },
        });
    }
    let subpath = if export == "default" {
        source.to_string()
    } else {
        format!("{source}/{export}")
    };
    let subpath = subpath.strip_prefix("node:").unwrap_or(&subpath);
    let submodule = perry_dispatch::native_submodule_exports::SUBMODULE_EXPORTS
        .iter()
        .find(|(module, _, _)| *module == subpath)
        .map(|(_, key, _)| *key);
    if submodule.is_none()
        && export != "default"
        && perry_api_manifest::is_node_core_module(source)
        && perry_api_manifest::module_has_public_named_export(source, export)
        && perry_dispatch::native_callable_exports::export_bucket(source).is_some()
    {
        return Some(PreparedExport::Callable {
            symbol: perry_dispatch::native_callable_exports::constant_value_symbol(source, export),
            observe: if observe_writes { "1" } else { "0" },
        });
    }
    let key = submodule?;
    let install = crate::nm_install::nm_submod_install_symbol(key)?;
    if export != "default" {
        let m = strings.intern(source);
        let p = strings.intern(export);
        return Some(PreparedExport::SubmoduleMember {
            install,
            source: format!("@{}", strings.entry(m).handle_global),
            member: format!("@{}", strings.entry(p).handle_global),
            helper: if observe_writes {
                "js_native_module_esm_export_value"
            } else {
                "js_native_module_named_esm_export_value"
            },
        });
    }
    let k = strings.intern(key);
    let d = strings.intern("default");
    Some(PreparedExport::Submodule {
        install,
        key: format!("@{}", strings.entry(k).bytes_global),
        key_len: key.len().to_string(),
        default: format!("@{}", strings.entry(d).bytes_global),
    })
}

/// Import metadata for registry-backed modules carries the declaration key.
pub(crate) fn prepare_submodule(
    strings: &mut StringPool,
    key: &str,
    export: &str,
    observe_writes: bool,
) -> Option<PreparedExport> {
    let (module, _, _) = perry_dispatch::native_submodule_exports::SUBMODULE_EXPORTS
        .iter()
        .find(|(_, candidate, _)| *candidate == key)?;
    prepare(strings, module, export, observe_writes)
}

/// Resolve the callee before evaluating its arguments, then keep it rooted
/// through their evaluation just as an ordinary first-class call does.
pub(crate) fn call(
    ctx: &mut crate::expr::FnCtx<'_>,
    export: PreparedExport,
    args: &[perry_hir::Expr],
) -> anyhow::Result<String> {
    let value = export.emit(ctx.block());
    let (value, args, group) =
        crate::lower_call::lower_args_after_emitted_rooted(ctx, &value, args)?;
    let bits = ctx.block().bitcast_double_to_i64(&value);
    let handle = ctx
        .block()
        .and(crate::types::I64, &bits, crate::nanbox::POINTER_MASK_I64);
    let result = crate::lower_call::emit_closure_handle_call(ctx, &handle, &args);
    group.release(ctx);
    Ok(result)
}
