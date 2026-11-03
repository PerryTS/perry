// HWS hook: js_nm_install_wasi
// HWS prefixes: perry_runtime::wasi::|js_wasi_
import { WASI } from "node:wasi"; const w=new WASI({version:"preview1"}); console.log(typeof w.wasiImport.args_get);

// This positive witness also exercises the complete module installer.
import * as moduleSurface from "node:wasi";
if (Object.keys(moduleSurface).length === 0) throw new Error("empty builtin namespace");
