use super::{compile_and_run, write};

// #11044: the node-builtin re-export fix (#10432/#10802/#10867) only
// special-cased `perry_api_manifest::is_node_core_module` sources. A local
// facade module re-exporting a named binding from a Perry-native NPM
// package that is NOT a node core builtin -- `bcrypt` here; `ws` was the
// original shape before npm `ws` compiled from source -- fell through to
// the generic `Export::ReExport` arm, which has no compiled source module
// to follow either. Codegen then expected a
// local function body for the forwarded name that was never emitted, and
// referencing it as a value inside a closure (constructing it dynamically)
// link-failed on an undefined `__perry_wrap_perry_fn_<facade>__<name>`
// symbol.
//
// This is exactly the shape of ethers' `src.ts/providers/ws.ts` (`export {
// WebSocket } from "ws";`), imported under a renamed local binding by
// `provider-websocket.ts` and referenced inside
// `this.#connect = () => { return new _WebSocket(url); }`.
#[test]
fn native_npm_package_reexports_survive_local_facade_modules() {
    let dir = tempfile::tempdir().expect("tempdir");
    write(
        dir.path(),
        "bcrypt_facade.ts",
        "export { hash } from 'bcrypt';\n",
    );
    write(
        dir.path(),
        "main.ts",
        "import { hash as _hash } from './bcrypt_facade';\n\
         class Connector {\n\
         \x20 #connect: () => any;\n\
         \x20 constructor(password: string) {\n\
         \x20   this.#connect = () => { return _hash(password, 4); };\n\
         \x20 }\n\
         \x20 hasConnector(): boolean { return typeof this.#connect === 'function'; }\n\
         }\n\
         const conn = new Connector('pw');\n\
         console.log('hasConnector:', conn.hasConnector());\n",
    );

    assert_eq!(
        compile_and_run(dir.path(), "main.ts"),
        "hasConnector: true\n"
    );
}
