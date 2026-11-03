//! Item-level native namespace declarations for `console` (A2-DESIGN §5): each item is the
//! module's keys in Node's order and what each one is; the realm producer reads
//! the source text, the vtable static it declares carries the keys to `Object.keys`.

crate::realm_decl! {
    namespace NM_CONSOLE = "console" {
        cell = "console";
        names = ["%NativeNamespace:console%", "node:console.default"];
        members = {
            log;
            info;
            debug;
            warn;
            error;
            dir;
            time;
            timeEnd;
            timeLog;
            trace;
            assert;
            clear;
            count;
            countReset;
            group;
            groupEnd;
            table;
            dirxml;
            groupCollapsed;
            Console;
            profile;
            profileEnd;
            timeStamp;
            context;
            createTask;
        }
    }
}
