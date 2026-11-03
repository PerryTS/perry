//! Item-level native namespace declarations for `tty` (A2-DESIGN §5): each item is the
//! module's keys in Node's order and what each one is; the realm producer reads
//! the source text, the vtable static it declares carries the keys to `Object.keys`.

crate::realm_decl! {
    namespace NM_TTY = "tty" {
        names = ["node:tty.default"];
        members = {
            isatty = (ref("node:tty.isatty"));
            ReadStream = (ref("node:tty.ReadStream"));
            WriteStream = (ref("node:tty.WriteStream"));
        }
    }
}
