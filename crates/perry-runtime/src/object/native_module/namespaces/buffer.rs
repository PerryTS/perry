//! Item-level native namespace declarations for `buffer` (A2-DESIGN §5): each item is the
//! module's keys in Node's order and what each one is; the realm producer reads
//! the source text, the vtable static it declares carries the keys to `Object.keys`.

crate::realm_decl! {
    namespace NM_BUFFER_CONSTANTS = "buffer.constants" {
        cell = "node:buffer.constants";
        names = ["node:buffer.default.constants"];
        members = {
            MAX_LENGTH = (9007199254740991.0);
            MAX_STRING_LENGTH = (536870888.0);
        }
    }
}

crate::realm_decl! {
    namespace NM_BUFFER = "buffer" {
        names = ["node:buffer.default"];
        members = {
            Buffer = (ref(Buffer));
            transcode = (undefined);
            isUtf8 = (ref("node:buffer.isUtf8"));
            isAscii = (ref("node:buffer.isAscii"));
            kMaxLength = (9007199254740991.0);
            kStringMaxLength = (536870888.0);
            btoa = (ref("node:buffer.btoa"));
            atob = (ref("node:buffer.atob"));
            constants;
            INSPECT_MAX_BYTES = (50.0);
            Blob = (ref(Blob));
            resolveObjectURL = (undefined);
            File = (ref(File));
        }
    }
}
