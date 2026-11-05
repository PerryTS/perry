// HWS hook: js_runtime_install_thread
// HWS prefixes: perry_runtime::thread::|perry_runtime::worker_threads::
import { isMainThread } from "node:worker_threads"; console.log(isMainThread);
