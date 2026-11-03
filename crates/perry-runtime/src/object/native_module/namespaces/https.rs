//! Item-level native namespace declarations for `https` (A2-DESIGN §5): each item is the
//! module's keys in Node's order and what each one is; the realm producer reads
//! the source text, the vtable static it declares carries the keys to `Object.keys`.

crate::realm_decl! {
    namespace NM_HTTPS = "https" {
        names = ["node:https.default"];
        members = {
            Agent = (ref("node:https.Agent"));
            Server = (ref("node:https.Server"));
            createServer = (ref("node:https.createServer"));
            get = (ref("node:https.get"));
            request = (ref("node:https.request"));
            globalAgent = (ref("node:https.globalAgent"));
        }
    }
}
