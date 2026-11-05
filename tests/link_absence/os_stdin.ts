// HWS companion: os
// HWS hook: js_runtime_install_os
// HWS prefixes: perry_runtime::os::|js_os_
// HWS stdin: "HWS input 42\n"
let text="";
const guard=setTimeout(()=>{console.log("missing stdin end");process.exit(1)},3000);
process.stdin.setEncoding("utf8");
process.stdin.on("data",chunk=>{text+=chunk});
process.stdin.on("end",()=>{clearTimeout(guard);console.log(text.trim())});
process.stdin.resume();
