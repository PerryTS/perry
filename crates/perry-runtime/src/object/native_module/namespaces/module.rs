//! Item-level native namespace declarations for `module` (A2-DESIGN §5): each item is the
//! module's keys in Node's order and what each one is; the realm producer reads
//! the source text, the vtable static it declares carries the keys to `Object.keys`.

crate::realm_decl! {
    namespace NM_MODULE = "module" {
        births = [Object];
        members = {
            Module = (ref("node:module.Module"));
            SourceMap = (ref("node:module.SourceMap"));
            _cache = (ref("node:module._cache"));
            _extensions = (ref("node:module._extensions"));
            _findPath = (ref("node:module._findPath"));
            _initPaths = (ref("node:module._initPaths"));
            _load = (ref("node:module._load"));
            _nodeModulePaths = (ref("node:module._nodeModulePaths"));
            _pathCache = (ref("node:module._pathCache"));
            _preloadModules = (ref("node:module._preloadModules"));
            _resolveFilename = (ref("node:module._resolveFilename"));
            _resolveLookupPaths = (ref("node:module._resolveLookupPaths"));
            builtinModules = (ref("node:module.builtinModules"));
            constants = (ref("node:module.constants"));
            createRequire = (ref("node:module.createRequire"));
            default = (ref("node:module"));
            enableCompileCache = (ref("node:module.enableCompileCache"));
            findPackageJSON = (ref("node:module.findPackageJSON"));
            findSourceMap = (ref("node:module.findSourceMap"));
            flushCompileCache = (ref("node:module.flushCompileCache"));
            getCompileCacheDir = (ref("node:module.getCompileCacheDir"));
            getSourceMapsSupport = (ref("node:module.getSourceMapsSupport"));
            globalPaths = (ref("node:module.globalPaths"));
            isBuiltin = (ref("node:module.isBuiltin"));
            register = (ref("node:module.register"));
            registerHooks = (ref("node:module.registerHooks"));
            runMain = (ref("node:module.runMain"));
            setSourceMapsSupport = (ref("node:module.setSourceMapsSupport"));
            stripTypeScriptTypes = (ref("node:module.stripTypeScriptTypes"));
            syncBuiltinESMExports = (ref("node:module.syncBuiltinESMExports"));
        }
    }
}
