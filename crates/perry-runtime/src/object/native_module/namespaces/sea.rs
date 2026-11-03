//! Item-level native namespace declarations for `sea` (A2-DESIGN §5): each item is the
//! module's keys in Node's order and what each one is; the realm producer reads
//! the source text, the vtable static it declares carries the keys to `Object.keys`.

crate::realm_decl! {
    namespace NM_SEA_DEFAULT = "sea.default" {
        cell = "node:sea.default";
        members = {
            isSea = (ref("node:sea.isSea"));
            getAsset = (ref("node:sea.getAsset"));
            getRawAsset = (ref("node:sea.getRawAsset"));
            getAssetAsBlob = (ref("node:sea.getAssetAsBlob"));
            getAssetKeys = (ref("node:sea.getAssetKeys"));
        }
    }
}
