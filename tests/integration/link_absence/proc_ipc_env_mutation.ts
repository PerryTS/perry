// HWS companion: proc_ipc
// HWS hook: js_runtime_install_proc_ipc
// HWS prefixes: perry_runtime::process::ipc::
// HWS ipc: {"payload":42}
const key=["NODE","CHANNEL","FD"].join("_");
const env:any=process.env;env[key]="user";console.log(env[key]);
const again:any=process.env;console.log(again[key]);console.log(process.env.NODE_CHANNEL_FD);
console.log(process.connected);
