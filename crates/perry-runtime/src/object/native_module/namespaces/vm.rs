//! Item-level native namespace declarations for `vm` (A2-DESIGN §5): each item is the
//! module's keys in Node's order and what each one is; the realm producer reads
//! the source text, the vtable static it declares carries the keys to `Object.keys`.

crate::realm_decl! {
    namespace NM_VM = "vm" {
        names = ["node:vm.default"];
        members = {
            Script = (ref("node:vm.Script"), attrs = writable | enumerable);
            createContext = (ref("node:vm.createContext"), attrs = writable | enumerable);
            createScript = (ref("node:vm.createScript"), attrs = writable | enumerable);
            runInContext = (ref("node:vm.runInContext"), attrs = writable | enumerable);
            runInNewContext = (ref("node:vm.runInNewContext"), attrs = writable | enumerable);
            runInThisContext = (ref("node:vm.runInThisContext"), attrs = writable | enumerable);
            isContext = (ref("node:vm.isContext"), attrs = writable | enumerable);
            compileFunction = (ref("node:vm.compileFunction"), attrs = writable | enumerable);
            measureMemory = (ref("node:vm.measureMemory"), attrs = writable | enumerable);
            constants = (ref("node:vm.constants"), attrs = writable | enumerable);
        }
    }
}
