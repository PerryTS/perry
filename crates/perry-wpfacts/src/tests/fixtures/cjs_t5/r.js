import { cache as __perry_cjs_cache, main as __perry_cjs_main, extensions as __perry_cjs_extensions, setPending as __perry_cjs_set_pending, takePending as __perry_cjs_take_pending, link as __perry_cjs_link, requireComputed as __perry_cjs_require_computed, requireError as __perry_cjs_require_error } from 'perry:cjs';
import { createRequire as __perry_cjs_create_require } from 'node:module';
import _req_0 from './b';
import { __perry_cjs_record as __perry_cjs_rec_0 } from './b';
export var __perry_cjs_record;
let __perry_cjs_lk_0 = false;
function __perry_require_static_0() {
    
        const record = __perry_cjs_init(__perry_cjs_rec_0);
        if (__perry_cjs_lk_0 === false) {
            __perry_cjs_lk_0 = true;
            if (record.parent === undefined) record.parent = __perry_cjs_record;
            __perry_cjs_record.children.push(record);
        }
        return record.exports;
    
}



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
    const __cjs_module = {
        exports: __perry_declared_keys_object("newInstance"),
        __perry_cjs_record: true,
        id: "/t5/r.js",
        path: "/t5",
        filename: "/t5/r.js",
        loaded: false,
        children: [],
        parent: undefined,
        paths: ["/t5" + '/node_modules'],
        require: undefined
    };
    // The module's record binding: what a static edge of a requiring module
    // reads, from before the body, so a cycle sees the partial record.
    __perry_cjs_record = __cjs_module;
    
    
    var module = __cjs_module;
    var exports = __cjs_module.exports;
    // The program's `require.cache` (the support module's object, whose keys
    // are the graph's module paths): this module's key is a slot.
    const __perry_cjs_c = __perry_cjs_cache;
    __perry_cjs_c["/t5/r.js"] = __cjs_module;

    // This module's own loader for the cold paths of `require` (a computed
    // specifier outside the graph, relative to this module), created on
    // first use: `createRequire` costs ~117k instructions. Only a program
    // that is not closed can compute a specifier or reload.
    let __perry_cjs_reload_require;
    function __perry_cjs_reloader() {
        
            throw __perry_cjs_require_error('error', 'ERR_PERRY_CJS_CLOSED', 'a closed program has no CommonJS loader');
        
    }
    function require(specifier) {
        if (typeof specifier !== 'string') throw __perry_cjs_require_error('type', 'ERR_INVALID_ARG_TYPE', 'The "id" argument must be of type string.');
        if (specifier === '') throw __perry_cjs_require_error('type', 'ERR_INVALID_ARG_VALUE', 'The argument "id" must be a non-empty string.');
        if (specifier === "./b") return __perry_require_static_0();
        // A specifier no case above resolved: builtins, native addons, a
        // compiled module reached by a computed path, a `.json` file, a
        // package outside the graph (`perry:cjs`'s `requireComputed`).
        // A closed program has no computed require and no reload (design
        // CJS_LINK §2.5): no loader surface is linked for it.
        
            throw __perry_cjs_require_error('error', 'MODULE_NOT_FOUND', "Cannot find module '" + specifier + "'");
        
    }
    // No `defineProperty(require, 'name', ...)`: a `function require(...)`
    // declaration already carries exactly
    // {value:'require', writable:false, enumerable:false, configurable:true},
    // verified identical in Node 26 and Perry. The redundant install also gave
    // the require object OBJ_FLAG_HAS_DESCRIPTORS, which pushed every later
    // `require.resolve = ...` / `require.cache = ...` assignment onto the
    // descriptor-bearing store path (#10287) in every CommonJS module.
    const __perry_cjs_resolve = function resolve(specifier, options) {
        if (typeof specifier !== 'string') throw __perry_cjs_require_error('type', 'ERR_INVALID_ARG_TYPE', 'The "request" argument must be of type string.');
        if (specifier === "./b") return "/t5/b.js";
        // `require.resolve` is reachable only through `require` as a value,
        // which a closed program does not have.
        
        throw __perry_cjs_require_error('error', 'MODULE_NOT_FOUND', 'Cannot find module ' + specifier);
    };
    __perry_cjs_resolve.paths = function paths(specifier) {
        if (typeof specifier !== 'string') throw __perry_cjs_require_error('type', 'ERR_INVALID_ARG_TYPE', 'The "request" argument must be of type string.');
        return null;
    };
    require.resolve = __perry_cjs_resolve;
    // `cache`, `extensions` and `main` are the program's (`perry:cjs`): Node's
    // loader reads `Module._cache`, never this property, so reassigning
    // `require.cache` does not redirect loading.
    require.cache = __perry_cjs_c;
    require.extensions = __perry_cjs_extensions;
    require.main = __perry_cjs_main;
    module.require = function moduleRequire(specifier) { return require(specifier); };

    const { B } = __perry_require_static_0();
const NONE = null;
class P {
  constructor(v) { this.v = v; this.link = NONE; }
  get() { return this.v; }
}
class S {
  constructor(n) { function z(y) { return y; } z.call(this, n); this.p = new P(n); }
}
class Q {
  constructor(n) { this.q = new P(n); }
}
function mk(n) { return new Q(n); }
class T extends B {
  constructor() { super(); this.k = NONE; }
  run() { function w(y) { return y; } w.call(this, 0); const s = new S(1); const q = mk(2); return s.p.get() + q.q.get() + s.p.v + q.q.v; }
}
exports.newInstance = () => new T();


    __cjs_module.loaded = true; 
    return __cjs_module.exports;
}
return __perry_cjs_factory();
})();

export default _cjs;


export const newInstance = _cjs.newInstance;

