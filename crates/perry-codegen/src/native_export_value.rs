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
    if let Some(symbol) =
        perry_dispatch::native_callable_exports::export_value_symbol(source, export)
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
    let key = crate::nm_install::native_namespace_submodule_key(&subpath)?;
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
