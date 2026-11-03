// HWS companion: bun_cli_utils
// HWS hook: js_runtime_install_bun_cli_utils
// HWS prefixes: perry_runtime::bun_compat::
// HWS node: bun_stdio.node.mjs
// HWS stdin: "HWS input 42\n"
const input=Bun.stdin;console.log((await input.text()).trim());
