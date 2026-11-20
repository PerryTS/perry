// HWS hook: js_runtime_install_intl_datetime
// HWS prefixes: jiff::tz::|jiff_tzdb::|perry_runtime::date::tzdb|timezone_provider::|perry_runtime::date::compiled_
console.log(new Intl.DateTimeFormat("en-US", { timeZone: "UTC", year: "numeric" }).format(new Date(0)));
