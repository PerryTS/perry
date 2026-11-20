// HWS companion: wasi
// HWS hook: js_nm_install_wasi
// HWS prefixes: perry_runtime::wasi::|js_wasi_
import * as wasi from "node:wasi";
const Parent=wasi.WASI;
class Child extends Parent {constructor(){super({version:"preview1",args:["hws"],env:{HWS:"42"}})}}
const instance=new Child();
console.log(instance instanceof wasi.WASI);
console.log(Object.keys(instance.getImportObject()).join(","));
console.log(typeof instance.wasiImport.args_get);

// This positive witness also exercises the complete module installer.
import * as moduleSurface from "node:wasi";
if (Object.keys(moduleSurface).length === 0) throw new Error("empty builtin namespace");
