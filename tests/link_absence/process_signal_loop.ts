// HWS hook: js_runtime_install_proc_ipc
// HWS prefixes: perry_runtime::process::ipc::
// HWS companion: proc_ipc
const deadline = setTimeout(() => { throw new Error("signal not delivered"); }, 1000);
process.once("SIGUSR1", () => { clearTimeout(deadline); console.log("signal"); });
process.kill(process.pid, "SIGUSR1");
