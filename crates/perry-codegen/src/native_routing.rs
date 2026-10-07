//! Which library serves each native module in this build.
//!
//! The compile driver decides this once per compile (from
//! `well_known_bindings.toml` and `PERRY_DISABLE_WELL_KNOWN`) and hands the
//! decision to codegen here before any module is lowered. The linker reads the
//! same value to choose the stdlib features and the wrapper archives, so the
//! two can no longer disagree: codegen emits a wrapper's symbols, including its
//! namespace install hook, only for a module the decision routes to that
//! wrapper, and the linker links exactly the wrappers the decision names.
//!
//! A module the decision does not mention is served by the bundled runtime /
//! stdlib, which is also what every module is when no driver has set a
//! decision (codegen used on its own, as its unit tests do). The one thing such
//! a module still cannot get is a wrapper install hook; its namespace installs
//! the runtime dispatch bucket.

use std::collections::BTreeMap;
use std::sync::RwLock;

/// Who serves one native module.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum NativeProvider {
    /// The well-known wrapper crate whose staticlib stem is this
    /// (`perry_ext_http`).
    Wrapper(String),
    /// The bundled runtime / stdlib, although the wrapper crate whose
    /// staticlib stem is this exists for the module
    /// (`PERRY_DISABLE_WELL_KNOWN=1`). A module whose wrapper defines symbols
    /// the bundled libraries lack has no provider at all
    /// ([`NativeRouting::unprovided`]), and a program importing it does not
    /// compile.
    Bundled(String),
}

impl NativeProvider {
    /// The wrapper crate's staticlib stem, whether or not it is linked.
    fn wrapper_lib(&self) -> &str {
        match self {
            NativeProvider::Wrapper(lib) | NativeProvider::Bundled(lib) => lib,
        }
    }
}

/// The routing decision for every well-known native module, keyed by the bare
/// module name (no `node:` prefix).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NativeRouting {
    providers: BTreeMap<String, NativeProvider>,
}

fn bare(module: &str) -> &str {
    module.strip_prefix("node:").unwrap_or(module)
}

impl NativeRouting {
    pub fn new(providers: impl IntoIterator<Item = (String, NativeProvider)>) -> Self {
        Self {
            providers: providers
                .into_iter()
                .map(|(module, provider)| (bare(&module).to_string(), provider))
                .collect(),
        }
    }

    /// The wrapper staticlib stem serving `module`, or `None` when the
    /// bundled libraries serve it.
    pub fn wrapper(&self, module: &str) -> Option<&str> {
        match self.providers.get(bare(module)) {
            Some(NativeProvider::Wrapper(lib)) => Some(lib),
            _ => None,
        }
    }

    /// The modules of `modules` that this build serves from a wrapper.
    pub fn routed<'a, I>(&self, modules: I) -> std::collections::BTreeSet<String>
    where
        I: IntoIterator<Item = &'a String>,
    {
        modules
            .into_iter()
            .filter(|module| self.wrapper(module).is_some())
            .cloned()
            .collect()
    }

    /// The install hook of the wrapper serving `module`, if that wrapper has
    /// one. A wrapper's hook registers its export dispatcher and then chains to
    /// the runtime bucket install.
    pub fn wrapper_install_hook(&self, module: &str) -> Option<&'static str> {
        crate::ext_registry::wrapper_install_hook(self.wrapper(module)?)
    }

    /// The symbol codegen calls where it materializes `module`'s namespace:
    /// the serving wrapper's install hook, else the runtime dispatch bucket's
    /// install, or `None` when the module has no dispatch bucket.
    pub fn install_symbol(&self, module: &str) -> Option<&'static str> {
        let bucket = crate::nm_install::runtime_bucket_install_symbol(module)?;
        Some(self.wrapper_install_hook(module).unwrap_or(bucket))
    }

    /// The wrapper install hooks for a program's native modules, sorted and
    /// deduplicated: the entry prologue calls each one, so module objects the
    /// runtime creates itself (a CommonJS `require('net')`) reach the
    /// wrapper's export dispatcher too. (#10428, #10429)
    pub fn wrapper_install_hooks<'a>(
        &self,
        modules: impl IntoIterator<Item = &'a str>,
    ) -> Vec<String> {
        let mut hooks: Vec<String> = modules
            .into_iter()
            .filter_map(|module| self.wrapper_install_hook(module))
            .map(str::to_string)
            .collect();
        hooks.sort_unstable();
        hooks.dedup();
        hooks
    }

    /// Is `module` left without a provider? True when this build serves it
    /// from the bundled libraries although the FFI registry names symbols
    /// that only its wrapper crate defines: the bundled runtime / stdlib does
    /// not implement those, and the wrapper archive is not linked. The
    /// registry files a crate's symbols under one of the modules it serves
    /// (perry-ext-http's under `http`, also for `https` and `http2`), so the
    /// question is asked of every module served by the same crate. The
    /// compile driver refuses a program that imports such a module.
    pub fn unprovided(&self, module: &str) -> bool {
        let Some(NativeProvider::Bundled(lib)) = self.providers.get(bare(module)) else {
            return false;
        };
        self.providers.iter().any(|(other, provider)| {
            provider.wrapper_lib() == lib && crate::ext_registry::wrapper_owns_symbols(other)
        })
    }

    /// May codegen emit a call to `symbol`? Not when the symbol is a wrapper's
    /// and this build leaves that wrapper's module without a provider
    /// ([`Self::unprovided`]): the archive defining it is not linked.
    pub fn serves(&self, symbol: &str) -> bool {
        match crate::ext_registry::well_known_owner_for_symbol(symbol) {
            Some(owner) => !self.unprovided(owner),
            None => true,
        }
    }

    /// Stable text form for the object-cache key.
    pub fn cache_key(&self) -> String {
        self.providers
            .iter()
            .map(|(module, provider)| match provider {
                NativeProvider::Wrapper(lib) => format!("{module}={lib}"),
                NativeProvider::Bundled(_) => format!("{module}=-"),
            })
            .collect::<Vec<_>>()
            .join("|")
    }
}

/// The current compile's decision. Set once by the compile driver before any
/// module codegen runs, and folded into the object-cache key: the same module
/// lowers differently depending on which library serves its imports.
static PROGRAM_NATIVE_ROUTING: RwLock<Option<NativeRouting>> = RwLock::new(None);

/// Record the compile's routing decision. See [`PROGRAM_NATIVE_ROUTING`].
pub fn set_program_native_routing(routing: NativeRouting) {
    *PROGRAM_NATIVE_ROUTING.write().unwrap() = Some(routing);
}

/// The compile's routing decision as an object-cache key component.
pub fn program_native_routing_key() -> String {
    with_program_routing(NativeRouting::cache_key)
}

/// Read the compile's routing decision through `f`.
pub(crate) fn with_program_routing<R>(f: impl FnOnce(&NativeRouting) -> R) -> R {
    let guard = PROGRAM_NATIVE_ROUTING.read().unwrap();
    match guard.as_ref() {
        Some(routing) => f(routing),
        None => f(&NativeRouting::default()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flip_on() -> NativeRouting {
        NativeRouting::new([
            (
                "http".into(),
                NativeProvider::Wrapper("perry_ext_http".into()),
            ),
            (
                "https".into(),
                NativeProvider::Wrapper("perry_ext_http".into()),
            ),
            (
                "net".into(),
                NativeProvider::Wrapper("perry_ext_net".into()),
            ),
            ("ws".into(), NativeProvider::Wrapper("perry_ext_ws".into())),
            (
                "zlib".into(),
                NativeProvider::Wrapper("perry_ext_zlib".into()),
            ),
        ])
    }

    fn bundled(lib: &str) -> NativeProvider {
        NativeProvider::Bundled(lib.into())
    }

    fn flip_off() -> NativeRouting {
        NativeRouting::new([
            ("http".into(), bundled("perry_ext_http")),
            ("https".into(), bundled("perry_ext_http")),
            (
                "net".into(),
                NativeProvider::Wrapper("perry_ext_net".into()),
            ),
            ("ws".into(), NativeProvider::Wrapper("perry_ext_ws".into())),
            (
                "zlib".into(),
                NativeProvider::Wrapper("perry_ext_zlib".into()),
            ),
        ])
    }

    #[test]
    fn routed_http_installs_the_wrapper_hook_and_bundled_http_the_bucket() {
        assert_eq!(
            flip_on().install_symbol("node:https"),
            Some("js_ext_http_nm_install")
        );
        assert_eq!(
            flip_off().install_symbol("node:http"),
            Some("js_nm_install_http")
        );
        assert_eq!(
            flip_off().install_symbol("net"),
            Some("js_ext_net_nm_install")
        );
        assert_eq!(flip_on().install_symbol("fs"), Some("js_nm_install_fs"));
        assert_eq!(
            flip_off().install_symbol("node:zlib"),
            Some("js_ext_zlib_nm_install")
        );
        let bundled_zlib = NativeRouting::new([("zlib".into(), bundled("perry_ext_zlib"))]);
        assert_eq!(
            bundled_zlib.install_symbol("zlib"),
            Some("js_nm_install_zlib")
        );
        assert_eq!(flip_on().install_symbol("not-a-module"), None);
    }

    #[test]
    fn bundled_modules_withhold_their_wrapper_symbols() {
        assert!(flip_on().serves("js_node_http_create_server_with_options"));
        assert!(!flip_off().serves("js_node_http_create_server_with_options"));
        assert!(!flip_off().serves("js_ext_http_nm_install"));
        assert!(flip_off().serves("js_ext_net_nm_install"));
        assert!(flip_off().serves("js_array_push_f64"));
    }

    #[test]
    fn a_bundled_module_whose_wrapper_owns_symbols_has_no_provider() {
        assert!(flip_off().unprovided("node:http"));
        assert!(flip_off().unprovided("https"));
        assert!(!flip_on().unprovided("node:http"));
        assert!(!flip_off().unprovided("net"));
        // Not part of the decision: the bundled runtime serves it.
        assert!(!flip_off().unprovided("fs"));
        // Bundled, and the registry names no symbol only the wrapper defines:
        // the stdlib's own implementation serves it.
        let bundled_bcrypt = NativeRouting::new([("bcrypt".into(), bundled("perry_ext_bcrypt"))]);
        assert!(!bundled_bcrypt.unprovided("bcrypt"));
        // Prefix-registry owners count too.
        let bundled_ts =
            NativeRouting::new([("typescript".into(), bundled("perry_ext_typescript"))]);
        assert!(bundled_ts.unprovided("typescript"));
    }

    #[test]
    fn entry_hooks_follow_the_decision() {
        let modules = ["fs", "net", "node:http", "https", "http2", "bun"];
        assert_eq!(
            flip_on().wrapper_install_hooks(modules),
            vec!["js_ext_http_nm_install", "js_ext_net_nm_install"]
        );
        assert_eq!(
            flip_off().wrapper_install_hooks(modules),
            vec!["js_ext_net_nm_install"]
        );
        assert!(flip_on()
            .wrapper_install_hooks(["fs", "path", "events"])
            .is_empty());
    }
}
