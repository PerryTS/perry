// HWS companion: temporal
// HWS hook: js_runtime_install_temporal
// HWS prefixes: perry_runtime::temporal::|temporal_rs::
function invoke(date: Date, key: string) { return date[key](); }
const key = "toTemporal" + "Instant";
console.log(invoke(new Date(0), key).epochMilliseconds);
console.log(new Date(1)[key]().epochMilliseconds);
