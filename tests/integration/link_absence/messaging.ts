// HWS hook: js_runtime_install_messaging
// HWS prefixes: perry_runtime::messaging::|js_message_|js_broadcast_
const {port1,port2}=new MessageChannel(); console.log(typeof port1.postMessage); port1.close(); port2.close();
