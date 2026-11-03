//! Item-level native namespace declarations for `punycode` (A2-DESIGN §5): each item is the
//! module's keys in Node's order and what each one is; the realm producer reads
//! the source text, the vtable static it declares carries the keys to `Object.keys`.

crate::realm_decl! {
    namespace NM_PUNYCODE = "punycode" {
        cell = "node:punycode";
        births = [Array, Error, Function, Object];
        members = {
            decode = (ref("node:punycode.decode"));
            default = (ref("%NativeNamespace:punycode.default%"));
            encode = (ref("node:punycode.encode"));
            toASCII = (ref("node:punycode.toASCII"));
            toUnicode = (ref("node:punycode.toUnicode"));
            ucs2 = (ref("%NativeNamespace:punycode.ucs2%"));
            version = ("2.1.0");
        }
    }
}

crate::realm_decl! {
    namespace NM_PUNYCODE_DEFAULT = "punycode.default" {
        members = {
            version = ("2.1.0");
            ucs2 = (ref("%NativeNamespace:punycode.ucs2%"));
            decode = (ref("node:punycode.decode"));
            encode = (ref("node:punycode.encode"));
            toASCII = (ref("node:punycode.toASCII"));
            toUnicode = (ref("node:punycode.toUnicode"));
        }
    }
}

crate::realm_decl! {
    namespace NM_PUNYCODE_UCS2 = "punycode.ucs2" {
        members = {
            decode = (ref("node:punycode.ucs2.decode"));
            encode = (ref("node:punycode.ucs2.encode"));
        }
    }
}
