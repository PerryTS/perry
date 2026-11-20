// Two-phase realm: the first touch of the realm is a named global read of a
// core name (`performance`), then core members that reach phase-B families:
// a mark is a PerformanceMark, whose constructor is never named before.
const t0 = performance.now();
const mark: any = performance.mark("realm-two-phase");
console.log("mark", mark.name, mark.entryType, typeof mark.startTime, t0 >= 0);
console.log("mark ctor", mark.constructor.name, Object.prototype.toString.call(mark));
const markProto = Object.getPrototypeOf(mark);
const entryProto = Object.getPrototypeOf(markProto);
console.log("mark chain", markProto === PerformanceMark.prototype, entryProto === PerformanceEntry.prototype, mark instanceof PerformanceEntry);
console.log("entry names", Object.getOwnPropertyNames(PerformanceEntry.prototype).join(","));
const measure: any = performance.measure("m", "realm-two-phase");
console.log("measure", measure.constructor === PerformanceMeasure, measure.entryType, measure instanceof PerformanceEntry);
console.log("entries", performance.getEntriesByName("realm-two-phase").length, performance.getEntriesByType("measure").length);
console.log("perf globals", typeof PerformanceObserver, typeof PerformanceObserverEntryList, typeof PerformanceResourceTiming, typeof Performance);
console.log("performance", Object.getPrototypeOf(performance) === Performance.prototype, Object.getPrototypeOf(Performance.prototype) === EventTarget.prototype);
console.log("observer types", Array.isArray(PerformanceObserver.supportedEntryTypes));
performance.clearMarks();
performance.clearMeasures();
// A number formatted with a locale reaches Intl without naming it.
console.log("locale", (1234567.891).toLocaleString("en-US"), "a".localeCompare("b"), typeof Intl.Collator);
console.log("date", new Date(0).toLocaleDateString("en-US", { timeZone: "UTC" }));
