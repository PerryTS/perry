// HWS companion: proc_ipc
// HWS hook: js_runtime_install_proc_ipc
// HWS prefixes: perry_runtime::process::ipc::
const event = process.argv[2];
process.once(event, (value: any) => {
  console.log(value.payload);
  process.exit(0);
});
console.log("ready");
