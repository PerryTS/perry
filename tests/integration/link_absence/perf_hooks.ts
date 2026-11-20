// HWS hook: js_runtime_install_perf_hooks
// HWS prefixes: perry_runtime::perf_hooks::|perry_runtime::perf_histogram::|js_perf_
import { performance } from "node:perf_hooks"; performance.mark("hws"); console.log(performance.getEntriesByName("hws").length);
