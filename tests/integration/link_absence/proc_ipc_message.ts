// HWS companion: proc_ipc
// HWS hook: js_runtime_install_proc_ipc
// HWS prefixes: perry_runtime::process::ipc::
// HWS argv: ["message"]
// HWS ipc: {"payload":42}
// HWS ipc reply: {"answer":43}
const deadline=setTimeout(()=>{throw new Error("IPC message not delivered")},3000);
process.once("disconnect",()=>{console.log("disconnected");clearTimeout(deadline);process.exit(0)});
process.once(process.argv[2],(value:any)=>{
  console.log(value.payload);
  process.send({answer:value.payload+1},(error:any)=>{if(error)throw error;console.log("sent");process.disconnect()});
});
console.log("ready");
