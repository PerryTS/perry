// HWS companion: temporal
// HWS hook: js_runtime_install_temporal
// HWS prefixes: perry_runtime::temporal::|temporal_rs::
const date:any=new Date(0);const parts=["to","Temporal","Instant"];const name=parts.join("");
const value=date[name]();console.log(String(value.epochMilliseconds));
