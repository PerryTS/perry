// HWS companion: perf_hooks
// HWS hook: js_runtime_install_perf_hooks
// HWS prefixes: perry_runtime::perf_hooks::|perry_runtime::perf_histogram::|js_perf_
import {performance,PerformanceObserver,createHistogram} from "node:perf_hooks";
const histogram=createHistogram();
const timed=performance.timerify((value:number)=>value+1,{histogram});
console.log(timed(41),histogram.count);
performance.mark("hws-start",{detail:{value:7}});
const mark=performance.getEntriesByName("hws-start")[0];console.log(mark.detail.value);
performance.clearMarks("hws-start");console.log(performance.getEntriesByName("hws-start").length);
