// HWS companion: mod_dgram
// HWS hook: js_runtime_install_mod_dgram
// HWS prefixes: perry_runtime::dgram::|perry_runtime::dgram_reactor::|js_dgram_
import {createSocket} from "node:dgram";
process.env.PERRY_DETERMINISTIC_NET="1";
const server=createSocket("udp4"),client=createSocket("udp4");
const deadline=setTimeout(()=>{throw new Error("UDP message not delivered")},3000);
server.on("message",buffer=>{console.log(buffer.toString());clearTimeout(deadline);server.close();client.close()});
server.bind(0,"127.0.0.1",()=>client.send("payload",server.address().port,"127.0.0.1"));
