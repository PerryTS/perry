// HWS companion: os
// HWS hook: js_runtime_install_os
// HWS prefixes: perry_runtime::os::|js_os_
process.once("hws-event",(value:number)=>console.log(value));
console.log(process.emit("hws-event",42));
console.log(process.emit("hws-event",43));
process.once("exit",code=>console.log("exit",code));
