// HWS companion: proc_ipc
// HWS hook: js_runtime_install_proc_ipc
// HWS prefixes: perry_runtime::process::ipc::
// HWS ipc driver: proc_ipc_advanced.node.mjs
const deadline=setTimeout(()=>{throw new Error("IPC array not delivered")},3000);
process.once("message",(value:any)=>{
 console.log(value.constructor.name,value.length,value[0],value[1]);
 clearTimeout(deadline);process.disconnect();
});
