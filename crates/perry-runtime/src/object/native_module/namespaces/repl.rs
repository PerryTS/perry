//! Item-level native namespace declarations for `repl` (A2-DESIGN §5): each item is the
//! module's keys in Node's order and what each one is; the realm producer reads
//! the source text, the vtable static it declares carries the keys to `Object.keys`.

crate::realm_decl! {
    namespace NM_REPL_DEFAULT = "repl.default" {
        cell = "node:repl.default";
        members = {
            REPLServer = (ref("node:repl.REPLServer"));
            REPL_MODE_SLOPPY = (Symbol.for("nodejs.repl.mode.sloppy"));
            REPL_MODE_STRICT = (Symbol.for("nodejs.repl.mode.strict"));
            Recoverable = (ref("node:repl.Recoverable"));
            builtinModules = (ref("node:module.builtinModules"));
            start = (ref("node:repl.start"));
        }
    }
}

crate::realm_decl! {
    namespace NM_REPL = "repl" {
        births = [Object];
        members = {
            REPLServer = (ref("node:repl.REPLServer"));
            REPL_MODE_SLOPPY = (Symbol.for("nodejs.repl.mode.sloppy"));
            REPL_MODE_STRICT = (Symbol.for("nodejs.repl.mode.strict"));
            Recoverable = (ref("node:repl.Recoverable"));
            builtinModules = (ref("node:module.builtinModules"));
            start = (ref("node:repl.start"));
        }
    }
}
