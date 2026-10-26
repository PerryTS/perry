// HWS hook: none (no hook yet)
// HWS prefixes: perry_runtime::wasi::|js_wasi_new
import { WASI } from "node:wasi"; const w=new WASI({version:"preview1"}); console.log(typeof w.wasiImport.args_get);
