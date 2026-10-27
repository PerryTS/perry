import { createRequire as __perry_cjs_support_create_require } from 'node:module';
export const cache = __perry_declared_keys("/t3/benchmark.js", "/t3/richards.js", "/t3/sa_Richards.js");
export const main = { exports: undefined, __perry_cjs_record: true, id: undefined, path: undefined, filename: undefined, loaded: false, children: undefined, parent: undefined, paths: undefined, require: undefined };
// The runtime's `Module._cache`, `process.mainModule` and
// `require.extensions` exist only for a program that can reach them (not
// closed): a closed program reads none of them by census.
export let extensions;

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
// A literal builtin `require` is a static native binding (#13296); only a
// computed specifier reaches this loader, and only a program that is not
// closed has one. A closed program links no `createRequire`.
export function builtin(specifier) {
    
        throw requireError('error', 'MODULE_NOT_FOUND', "Cannot find module '" + specifier + "'");
    
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
    
        return value;
    
}
export function requireError(kind, code, message) {
    const err = kind === 'type' ? new TypeError(message) : new Error(message);
    err.code = code;
    return err;
}
export function requireComputed(specifier, dir, reloader) {
    // A closed program has no computed require (design CJS_LINK §2.5).
    
        throw requireError('error', 'MODULE_NOT_FOUND', "Cannot find module '" + specifier + "'");
    
}
