//! Item-level native namespace declarations for `readline` (A2-DESIGN §5): each item is the
//! module's keys in Node's order and what each one is; the realm producer reads
//! the source text, the vtable static it declares carries the keys to `Object.keys`.

crate::realm_decl! {
    namespace NM_READLINE = "readline" {
        names = ["node:readline.default"];
        members = {
            clearLine = (ref("node:readline.clearLine"));
            clearScreenDown = (ref("node:readline.clearScreenDown"));
            createInterface = (ref("node:readline.createInterface"));
            cursorTo = (ref("node:readline.cursorTo"));
            emitKeypressEvents = (ref("node:readline.emitKeypressEvents"));
            moveCursor = (ref("node:readline.moveCursor"));
            promises = (ref("node:readline.promises"));
        }
    }
}
