//! Item-level native namespace declarations for `domain` (A2-DESIGN §5): each item is the
//! module's keys in Node's order and what each one is; the realm producer reads
//! the source text, the vtable static it declares carries the keys to `Object.keys`.

crate::realm_decl! {
    namespace NM_DOMAIN = "domain" {
        names = ["node:domain.default"];
        members = {
            _stack;
            Domain = (ref("node:domain.Domain"));
            createDomain = (ref("node:domain.create"));
            create = (ref("node:domain.create"));
            active = (null);
        }
    }
}
