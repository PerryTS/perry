// HWS companion: thread
// HWS hook: js_runtime_install_thread
// HWS prefixes: perry_runtime::thread::
// HWS node: thread_spawn.node.mjs
import {spawn} from "perry/thread";
spawn(()=>42).then(value=>console.log(value));
