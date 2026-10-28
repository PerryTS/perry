// HWS hook: js_stdlib_install_external_net_pump
// HWS prefixes: perry_runtime::turnloop_net::|turnloop_net::
import { createServer } from "node:net"; const s=createServer(); s.listen(0,"127.0.0.1",()=>{console.log("listening"); s.close();});
