import { createRequire as __perry_cjs_support_create_require, isBuiltin as __perry_cjs_support_is_builtin } from 'node:module';
export const cache = __perry_declared_keys("/t3/sa_Richards.js", "/t3/richards.js", "/t3/benchmark.js");
export const main = { exports: undefined, __perry_cjs_record: true, id: undefined, path: undefined, filename: undefined, loaded: false, children: undefined, parent: undefined, paths: undefined, require: undefined };
export const extensions = __perry_cjs_install(cache, main);
let pendingParent;
export function setPending(record) {
    pendingParent = record;
}
export function takePending() {
    const record = pendingParent;
    pendingParent = undefined;
    return record;
}
let base;
export function builtin(specifier) {
    if (base === undefined) base = __perry_cjs_support_create_require("/t3/sa_Richards.js");
    return base(specifier);
}
// Node's updateChildren: a required child is linked once to its first parent
// and appears once in each requiring module's children.
export function link(module, child) {
    if (!child) return;
    if (child.parent === undefined) child.parent = module;
    if (module.children.indexOf(child) === -1) module.children.push(child);
}
// A path module's live record, or its fresh evaluation after a deletion
// through the requiring module's own loader. (A wrapper's static cases do the
// same with their literal path, so each reads its own cache slot.)
function refresh(path, value, reloader) {
    const cached = cache[path];
    if (cached !== undefined) return cached.exports;
    if (!__perry_has_path_module(path)) return value;
    return reloader()(path);
}
export function requireError(kind, code, message) {
    const err = kind === 'type' ? new TypeError(message) : new Error(message);
    err.code = code;
    return err;
}
export function requireComputed(specifier, dir, reloader) {
    // Builtins not hoisted as static imports resolve through createRequire,
    // which calls js_create_native_module_namespace, as Node's
    // require("process") does. The original spelling is kept: prefix-only
    // builtins are packages when requested without `node:` (#10410).
    if (__perry_cjs_support_is_builtin(specifier)) return builtin(specifier);
    // Native addons resolve through the same authenticated manifest and cache
    // as createRequire and process.dlopen, including computed paths.
    if (specifier.slice(-5) === '.node') return reloader()(specifier);
    // A module Perry compiled but that is reachable only by a computed path.
    // Next's webpack runtime uses absolute page paths and relative chunk
    // paths (`./chunks/` + id): a relative specifier joins the requiring
    // module's directory, as Node's per-module `require` does. The `./`
    // prefix is stripped textually: canonicalization only normalizes a path
    // that exists on disk, and deployed images may lack the sources.
    let path = specifier;
    if (specifier.charCodeAt(0) === 46) {
        if (specifier.charCodeAt(1) === 47) {
            path = dir + '/' + specifier.slice(2);
        } else if (specifier.charCodeAt(1) === 46 && specifier.charCodeAt(2) === 47) {
            path = dir + '/' + specifier;
        } else if (specifier === '.' || specifier === '..') {
            // Bare directory specifiers resolve through the directory's
            // `index.js` / package `main` (`js_require_path_module`'s
            // directory-candidate fallback).
            path = dir + '/' + specifier;
        }
    }
    const pathModule = __perry_require_path_module(path);
    if (pathModule !== undefined || __perry_has_path_module(path)) return refresh(path, pathModule, reloader);
    // An absolute `.json` path computed at run time (Next.js manifests) is
    // read and parsed; it is data, never code.
    if ((specifier.charCodeAt(0) === 47 || (specifier.length > 2 && specifier.charCodeAt(1) === 58)) && specifier.slice(-5) === '.json') {
        return __perry_require_json_disk(specifier);
    }
    // A computed bare platform package can name a manifest-listed addon
    // through package.json main, without a .node suffix in the request.
    return reloader()(specifier);
}
