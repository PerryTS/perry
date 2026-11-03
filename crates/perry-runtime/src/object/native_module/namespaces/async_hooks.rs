//! Item-level native namespace declarations for `async_hooks` (A2-DESIGN §5): each item is the
//! module's keys in Node's order and what each one is; the realm producer reads
//! the source text, the vtable static it declares carries the keys to `Object.keys`.

crate::realm_decl! {
    namespace NM_ASYNC_HOOKS_DEFAULT = "async_hooks.default" {
        cell = "node:async_hooks.default";
        members = {
            AsyncLocalStorage = (ref("node:async_hooks.AsyncLocalStorage"));
            createHook = (ref("node:async_hooks.createHook"));
            executionAsyncId = (ref("node:async_hooks.executionAsyncId"));
            triggerAsyncId = (ref("node:async_hooks.triggerAsyncId"));
            executionAsyncResource = (ref("node:async_hooks.executionAsyncResource"));
            asyncWrapProviders = (ref("node:async_hooks.asyncWrapProviders"));
            AsyncResource = (ref("node:async_hooks.AsyncResource"));
        }
    }
}
