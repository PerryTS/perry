// HWS companion: url_engine
// HWS hook: js_runtime_install_url_engine
// HWS prefixes: url::|perry_runtime::url::
import {createRequire} from "node:module";
const load=createRequire("file://"+process.cwd()+"/main.ts");
console.log(load("./value.json").answer);
