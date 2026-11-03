//! Item-level native namespace declarations for `querystring` (A2-DESIGN §5): each item is the
//! module's keys in Node's order and what each one is; the realm producer reads
//! the source text, the vtable static it declares carries the keys to `Object.keys`.

crate::realm_decl! {
    namespace NM_QUERYSTRING_DEFAULT = "querystring.default" {
        cell = "node:querystring.default";
        members = {
            unescapeBuffer = (ref("node:querystring.unescapeBuffer"));
            unescape = (ref("node:querystring.unescape"));
            escape = (ref("node:querystring.escape"));
            stringify = (ref("node:querystring.encode"));
            encode = (ref("node:querystring.encode"));
            parse = (ref("node:querystring.decode"));
            decode = (ref("node:querystring.decode"));
        }
    }
}
