// HWS hook: js_runtime_install_dyn_eval
// HWS prefixes: perry_runtime::dyn_eval::|perry_runtime::dyn_eval_hooks::
import { runInNewContext } from "node:vm"; console.log(runInNewContext("6 * 7"));
