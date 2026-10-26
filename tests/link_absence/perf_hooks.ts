// HWS hook: none (no hook yet)
// HWS prefixes: perry_runtime::perf_hooks::|js_performance_
import { performance } from "node:perf_hooks"; performance.mark("hws"); console.log(performance.getEntriesByName("hws").length);
