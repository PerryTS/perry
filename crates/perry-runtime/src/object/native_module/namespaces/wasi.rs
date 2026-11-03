//! Item-level native namespace declarations for `wasi` (A2-DESIGN §5): each item is the
//! module's keys in Node's order and what each one is; the realm producer reads
//! the source text, the vtable static it declares carries the keys to `Object.keys`.

crate::realm_decl! {
    namespace NM_WASI_DEFAULT = "wasi.default" {
        cell = "node:wasi.default";
        members = {
            WASI = (ref("node:wasi.WASI"));
        }
    }
}

crate::realm_decl! {
    namespace NM_WASI = "wasi" {
        births = [Object];
        members = {
            WASI = (ref("node:wasi.WASI"));
            default = (ref("node:wasi.default"));
        }
    }
}
