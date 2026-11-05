// HWS companion: string_normalize
// HWS hook: js_runtime_install_string_normalize
// HWS prefixes: unicode_normalization::|perry_runtime::string::normalize_hooked::
console.log("I\u0307".toLocaleLowerCase("tr"));
console.log("i\u0307".toLocaleUpperCase("lt"));
console.log("é".normalize("NFD").length);
