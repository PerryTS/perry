import { cache as __perry_cjs_cache, main as __perry_cjs_main, extensions as __perry_cjs_extensions, setPending as __perry_cjs_set_pending, takePending as __perry_cjs_take_pending, builtin as __perry_cjs_builtin, link as __perry_cjs_link, requireComputed as __perry_cjs_require_computed, requireError as __perry_cjs_require_error } from 'perry:cjs';
import { createRequire as __perry_cjs_create_require, isBuiltin as __perry_cjs_is_builtin } from 'node:module';


class Benchmark {
  innerBenchmarkLoop(innerIterations) {
    for (let i = 0; i < innerIterations; i += 1) {
      if (!this.verifyResult(this.benchmark())) {
        return false;
      }
    }
    return true;
  }

  benchmark() {
    throw new Error('subclass responsibility');
  }

  verifyResult() {
    throw new Error('subclass responsibility');
  }
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
        exports: __perry_declared_keys_object("Benchmark"),
        __perry_cjs_record: true,
        id: "/t3/benchmark.js",
        path: "/t3",
        filename: "/t3/benchmark.js",
        loaded: false,
        children: [],
        parent: __perry_cjs_take_pending(),
        paths: ["/t3" + '/node_modules'],
        require: undefined
    };
    // Node populates `module.parent` before the body evaluates, so link it
    // here rather than at the tail's registry publication.
    __perry_link_path_module_parent(__cjs_module);
    // Publish the record before user code, just as at final publication.
    // #10178: esbuild replaces module.exports before requiring its peer.
    // Holding the initial empty object loses that replacement at re-entry;
    // the runtime's existing record unwrap reads the current exports instead.
    __perry_register_path_module_partial("/t3/benchmark.js", __cjs_module, __perry_cjs_factory);
    var module = __cjs_module;
    var exports = __cjs_module.exports;
    // The program's `require.cache` (the support module's object, whose keys
    // are the graph's module paths): this module's key is a slot.
    const __perry_cjs_c = __perry_cjs_cache;
    __perry_cjs_c["/t3/benchmark.js"] = __cjs_module;

    // This module's own loader for the cold paths (a reload after
    // `delete require.cache[...]`, a computed specifier outside the graph),
    // created on first use: `createRequire` costs ~117k instructions.
    let __perry_cjs_reload_require;
    function __perry_cjs_reloader() {
        if (__perry_cjs_reload_require === undefined) __perry_cjs_reload_require = __perry_cjs_create_require("/t3/benchmark.js");
        return __perry_cjs_reload_require;
    }
    function require(specifier) {
        if (typeof specifier !== 'string') throw __perry_cjs_require_error('type', 'ERR_INVALID_ARG_TYPE', 'The "id" argument must be of type string.');
        if (specifier === '') throw __perry_cjs_require_error('type', 'ERR_INVALID_ARG_VALUE', 'The argument "id" must be a non-empty string.');

        // A specifier no case above resolved: builtins, native addons, a
        // compiled module reached by a computed path, a `.json` file, a
        // package outside the graph (`perry:cjs`'s `requireComputed`).
        return __perry_cjs_require_computed(specifier, "/t3", __perry_cjs_reloader);
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

        if (__perry_cjs_is_builtin(specifier)) return specifier;
        var __perry_nm_resolved = __perry_require_resolve_node_modules("/t3", specifier);
        if (__perry_nm_resolved !== undefined) return __perry_nm_resolved;
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

    // @ts-check
// This code is derived from the SOM benchmarks, see AUTHORS.md file.
//
// Copyright (c) 2015-2016 Stefan Marr <git@stefan-marr.de>
//
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the 'Software'), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED 'AS IS', WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
// THE SOFTWARE.

                 
                                       
                                                  
                                                 
                     
       
     
                
   

               
                                               
   

                  
                                               
   
 

exports.Benchmark = Benchmark;


    __cjs_module.loaded = true; __perry_register_path_module("/t3/benchmark.js", __cjs_module);
    return __cjs_module.exports;
}
return __perry_cjs_factory();
})();

export default _cjs;
export { Benchmark };




