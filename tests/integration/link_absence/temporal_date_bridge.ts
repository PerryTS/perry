// HWS companion: temporal
// HWS hook: js_runtime_install_temporal
// HWS prefixes: perry_runtime::temporal::|temporal_rs::
const date=new Date(0);const method=date.toTemporalInstant;
const value=method.call(date);console.log(String(value.epochMilliseconds));
