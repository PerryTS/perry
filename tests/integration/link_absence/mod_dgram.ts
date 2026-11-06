// HWS hook: js_runtime_install_mod_dgram
// HWS prefixes: perry_runtime::dgram::|perry_runtime::dgram_reactor::|js_dgram_
import { createSocket } from "node:dgram"; const s=createSocket("udp4"); s.close(); console.log("closed");
