Named re-exports of a Perry-native npm package (not a Node core builtin --
`ws`, `ioredis`, `mysql2`, ...) through a local facade module now compile and
link. `export { X } from "<native-package>"` previously fell through to the
generic re-export path, which has no compiled source module to follow for a
natively-intercepted package; codegen then expected a local function body for
the forwarded name that was never emitted, and referencing the binding as a
value (e.g. inside a closure) link-failed on an undefined
`__perry_wrap_perry_fn_<facade>__<name>` symbol. The synthetic-import
treatment that #10802/#10867 added for Node builtin re-exports is now applied
to any recognized Perry-native module source, not only `is_node_core_module`
ones. This unblocks ethers' `src.ts/providers/ws.ts` (`export { WebSocket }
from "ws";`), which `provider-websocket.ts` imports under a renamed local
binding and references inside a closure (#11044).
