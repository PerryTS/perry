//! Item-level native namespace declarations for `string_decoder` (A2-DESIGN §5): each item is the
//! module's keys in Node's order and what each one is; the realm producer reads
//! the source text, the vtable static it declares carries the keys to `Object.keys`.

crate::realm_decl! {
    namespace NM_STRING_DECODER = "string_decoder" {
        names = ["node:string_decoder.default"];
        members = {
            StringDecoder = (ref("node:string_decoder.StringDecoder"));
        }
    }
}
