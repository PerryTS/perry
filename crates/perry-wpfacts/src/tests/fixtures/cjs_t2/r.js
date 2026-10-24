import { createRequire as __perry_cjs_create_require, isBuiltin as __perry_cjs_is_builtin } from 'node:module';



const _cjs = (function() {
function __perry_cjs_factory() {
    // #3527: `module`/`exports` are reassignable `var`s (mirroring Node, where
    // they are wrapper-function parameters), so CJS bodies that do
    // `var module = X` / `module = X` / `exports = X` — e.g. iconv-lite's
    // `for (...) { var module = modules[i]; mergeModules(exports, module); }`
    // — rebind the local instead of colliding with a `const`. The stable
    // `__cjs_module` is what the module actually exports, read back at the end;
    // a body reassigning its local `module` can't clobber it (Node holds the
    // real module ref the same way), so named/default-export resolution stays
    // correct regardless of what the body does to its `module` local.
    // #6769: the Node `Module` record surface. Set before user code so a
    // recursive load of this module observes the same shape Node exposes.
    //
    // ONE runtime call, so the record is born with its final layout
    // (`CJS_MODULE_RECORD`): Node's keys, the `__perry_cjs_record` marker, and
    // a private slot for the factory that no script can name. As sequential
    // assignments it walked a shape transition and a cold property store per
    // key, about 10k instructions each at module-init time, in every CommonJS
    // module in the graph. `exports`, `children` and `paths` stay the
    // wrapper's own literals.
    //
    // `cjs_scaffolding.rs`'s `record_binding` matches this call. Changing it
    // drops the record back to being reported as a denied user candidate in
    // the `Ptr<Shape>` report; `preamble_canary_tests` is what catches that.
    const __cjs_module = __perry_cjs_module_record(
        {},
        "/t2/r.js",
        "/t2",
        [],
        globalThis.__perry_cjs_pending_parent,
        ["/t2" + '/node_modules'],
        __perry_cjs_factory
    );
    globalThis.__perry_cjs_pending_parent = undefined;
    // Node populates `module.parent` before the body evaluates, so link it
    // here rather than at the tail's registry publication.
    __perry_link_path_module_parent(__cjs_module);
    // Publish the record before user code, just as at final publication.
    // #10178: esbuild replaces module.exports before requiring its peer.
    // Holding the initial empty object loses that replacement at re-entry;
    // the runtime's existing record unwrap reads the current exports instead.
    // This needs no getter enumeration, copying, or extra publication calls.
    __perry_register_path_module_partial("/t2/r.js", __cjs_module);
    var module = __cjs_module;
    var exports = __cjs_module.exports;
    // One `createRequire` instance for the whole program, not one per module.
    // It is used only for `.cache`, `.extensions` and loading builtins, and all
    // three are process-global in Node — nothing here is bound to this
    // module's path. The call costs ~117k instructions, so paying it per module
    // cost OpenCode's ~2,200 CJS modules a quarter of a billion instructions
    // before any user code ran.
    const __perry_cjs_base_require = (globalThis.__perry_cjs_shared_require
        || (globalThis.__perry_cjs_shared_require = __perry_cjs_create_require("/t2/r.js")));
    __perry_cjs_base_require.cache["/t2/r.js"] = __cjs_module;
    function __perry_cjs_require_error(kind, code, message) {
        const err = kind === 'type' ? new TypeError(message) : new Error(message);
        err.code = code;
        return err;
    }
    // Keep the original spelling: prefix-only builtins are packages when
    // requested without `node:` (#10410).
    function __perry_cjs_require_is_builtin(specifier) {
        return __perry_cjs_is_builtin(specifier);
    }
    // `isBuiltin` comes from `node:module` instead of a switch emitted into
    // EVERY CommonJS module. The switch carried both spellings of all 58
    // builtin names, so each module interned ~120 string constants and
    // initialised its own copy of the table before running a line of user
    // code. It was also more permissive than Node: `sea`, `sqlite`, `test` and
    // `test/reporters` are builtins only in their `node:` form, and the switch
    // accepted the bare spelling too. The runtime predicate agrees with Node
    // 26 on all 58 names in both spellings.

    // A path-registry initializer runs once, while deleting require.cache
    // requests another execution of a loaded CommonJS factory. Allocate this
    // module-bound loader only on that cold path; ordinary loads keep the
    // registry and live-record fast paths above.
    let __perry_cjs_reload_require;
    function __perry_cjs_refresh_path(path, value) {
        const cached = __perry_cjs_base_require.cache[path];
        if (cached !== undefined) return cached.exports;
        if (!__perry_has_path_module(path)) return value;
        if (__perry_cjs_reload_require === undefined) __perry_cjs_reload_require = __perry_cjs_create_require("/t2/r.js");
        return __perry_cjs_reload_require(path);
    }
    function require(specifier) {
        if (typeof specifier !== 'string') throw __perry_cjs_require_error('type', 'ERR_INVALID_ARG_TYPE', 'The "id" argument must be of type string.');
        if (specifier === '') throw __perry_cjs_require_error('type', 'ERR_INVALID_ARG_VALUE', 'The argument "id" must be a non-empty string.');

        // #sdxgen: Node.js built-in modules that were NOT hoisted as static
        // imports (see the builtin_requires filter above). Resolve them via
        // createRequire at runtime, which calls js_create_native_module_namespace
        // under the hood — the same path Node.js uses for require("process").
        if (__perry_cjs_require_is_builtin(specifier)) {
            return __perry_cjs_base_require(specifier);
        }
        // Native addons resolve through the same authenticated manifest and
        // cache as createRequire and process.dlopen, including computed paths.
        if (specifier.slice(-5) === '.node') {
            if (__perry_cjs_reload_require === undefined) __perry_cjs_reload_require = __perry_cjs_create_require("/t2/r.js");
            return __perry_cjs_reload_require(specifier);
        }
        // Runtime `require(path)` of a module Perry AOT-compiled but that is
        // only reachable via a computed path. Next's webpack runtime uses both
        // absolute page paths and relative chunk paths (`./chunks/` + id).
        // Resolve the latter against this CJS module's directory before probing
        // the path registry, mirroring Node's per-module `require` binding.
        // `js_require_path_module` canonicalizes the joined path, so `./` and
        // `../` segments need no source-level normalization here.
        {
            // A runtime-COMPUTED *relative* specifier never matches that
            // registry, which is keyed by absolute source path. Next's
            // production webpack runtime does exactly this — `.next/server/
            // webpack-runtime.js` calls `require("./chunks/" + g.u(a))` — so
            // every lazy chunk missed and the App Route died at startup with
            // `Cannot find module './chunks/2.js'` even though that chunk WAS
            // compiled into the image. Statically-known relative specifiers are
            // already handled by the cases above; only computed ones reach
            // here, so join them against this module's own directory.
            //
            // The `./` prefix is stripped textually rather than left to
            // `std::fs::canonicalize`: that only normalizes a path that exists
            // on disk, and registration falls back to the raw string when it
            // does not, so `<dir>/./chunks/2.js` would miss `<dir>/chunks/2.js`
            // in exactly the deployed case where the sources are absent.
            var __perry_path_spec = specifier;
            if (specifier.charCodeAt(0) === 46) {
                if (specifier.charCodeAt(1) === 47) {
                    __perry_path_spec = "/t2" + '/' + specifier.slice(2);
                } else if (specifier.charCodeAt(1) === 46 && specifier.charCodeAt(2) === 47) {
                    __perry_path_spec = "/t2" + '/' + specifier;
                } else if (specifier === '.' || specifier === '..') {
                    // The bare directory specifiers carry no trailing
                    // separator, so the two prefix tests above miss them —
                    // yet Node accepts `require('.')` / `require('..')` and
                    // resolves them through the directory's `index.js` /
                    // package `main`, which `js_require_path_module` also
                    // does via its directory-candidate fallback. Without the
                    // join the key stays a bare `.` and can never hit.
                    __perry_path_spec = "/t2" + '/' + specifier;
                }
            }
            const __perry_path_mod = __perry_require_path_module(__perry_path_spec);
            if (__perry_path_mod !== undefined || __perry_has_path_module(__perry_path_spec)) return __perry_cjs_refresh_path(__perry_path_spec, __perry_path_mod);
        }
        // Runtime `require(absolutePath)` of a `.json` file (Next.js loads
        // manifests this way: `require(this.middlewareManifestPath)`). Node's
        // require reads + JSON.parses `.json` files; the statically-resolved
        // cases above only cover specifiers known at compile time, so a path
        // computed at runtime falls here. `.json` is pure data (no eval), so we
        // read it from disk and parse it. Uncompiled `.js` runtime require
        // stays unsupported — that would require evaluating arbitrary code.
        if ((specifier.charCodeAt(0) === 47 || (specifier.length > 2 && specifier.charCodeAt(1) === 58)) && specifier.slice(-5) === '.json') {
            return __perry_require_json_disk(specifier);
        }
        // A computed bare platform package can name a manifest-listed addon
        // through package.json main, without a .node suffix in the request.
        if (__perry_cjs_reload_require === undefined) __perry_cjs_reload_require = __perry_cjs_create_require("/t2/r.js");
        return __perry_cjs_reload_require(specifier);
    }
    // No `defineProperty(require, 'name', ...)`: a `function require(...)`
    // declaration already carries exactly
    // {value:'require', writable:false, enumerable:false, configurable:true},
    // verified identical in Node 26 and Perry. The redundant install also gave
    // the require object OBJ_FLAG_HAS_DESCRIPTORS, which pushed every later
    // `require.resolve = ...` / `require.cache = ...` assignment onto the
    // descriptor-bearing store path (#10287) in every CommonJS module.
    require.resolve = function resolve(specifier, options) {
        if (typeof specifier !== 'string') throw __perry_cjs_require_error('type', 'ERR_INVALID_ARG_TYPE', 'The "request" argument must be of type string.');

        if (__perry_cjs_require_is_builtin(specifier)) return specifier;
        var __perry_nm_resolved = __perry_require_resolve_node_modules("/t2", specifier);
        if (__perry_nm_resolved !== undefined) return __perry_nm_resolved;
        throw __perry_cjs_require_error('error', 'MODULE_NOT_FOUND', 'Cannot find module ' + specifier);
    };
    require.resolve.paths = function paths(specifier) {
        if (typeof specifier !== 'string') throw __perry_cjs_require_error('type', 'ERR_INVALID_ARG_TYPE', 'The "request" argument must be of type string.');
        return null;
    };
    // `cache` and `extensions` come straight from the createRequire instance:
    // the placeholder object literals they used to be initialised with were
    // overwritten on the very next line, so every CJS module allocated an
    // object plus three closures and immediately dropped them. At OpenCode's
    // ~2,200 CJS modules that is pure startup garbage.
    require.cache = __perry_cjs_base_require.cache;
    require.extensions = __perry_cjs_base_require.extensions;
    require.main = __perry_get_cjs_main_module();
    module.require = function moduleRequire(specifier) { return require(specifier); };

    const NONE = null;
class R {
  append(p, q) {
    p.link = NONE;
    if (NONE === q) return p;
    let m = q;
    while (NONE !== m.link) m = m.link;
    m.link = p;
    return q;
  }
}
class S extends R {
  constructor(v) { super(); this.v = v; this.link = NONE; }
  get() { return this.v; }
  static make(v) { return new S(v); }
}
class T extends R {
  constructor() { super(); this.k = S; }
  run() { const a = this.k.make(1); const b = S.make(2); a.append(b, NONE); return a.get() + b.get(); }
}
exports.newInstance = () => new T();


    __cjs_module.loaded = true; __perry_register_path_module("/t2/r.js", __cjs_module);
    return __cjs_module.exports;
}
return __perry_cjs_factory();
})();

export default _cjs;


export const newInstance = _cjs.newInstance;


