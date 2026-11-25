// A static member read through the namespace mints PerformanceObserver before
// any construction; the constructor still carries its prototype methods and
// its supportedEntryTypes accessor, and observe() reaches the observer.
import * as perf_hooks from "node:perf_hooks";

console.log(typeof perf_hooks.PerformanceObserver.supportedEntryTypes);
const observer = new perf_hooks.PerformanceObserver(() => {});
observer.observe({ entryTypes: ["mark"] });
observer.disconnect();
console.log("observed");

const Observer: any = perf_hooks.PerformanceObserver;
console.log(Object.getOwnPropertyNames(Observer.prototype).join(","));
console.log(typeof Object.getOwnPropertyDescriptor(Observer, "supportedEntryTypes")?.get);
console.log(Object.getPrototypeOf(observer) === Observer.prototype);
