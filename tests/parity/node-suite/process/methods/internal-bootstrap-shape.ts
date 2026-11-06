import process from "node:process";

function logHelper(name: string, value: any) {
  const length = typeof value === "function" ? value.length : "n/a";
  console.log(
    "helper:",
    name,
    Object.prototype.propertyIsEnumerable.call(process, name),
    typeof value,
    length,
  );
}

logHelper("binding", (process as any).binding);
logHelper("_linkedBinding", (process as any)._linkedBinding);
logHelper("dlopen", (process as any).dlopen);
logHelper("_rawDebug", (process as any)._rawDebug);
logHelper("_debugProcess", (process as any)._debugProcess);
logHelper("_debugEnd", (process as any)._debugEnd);
logHelper("_startProfilerIdleNotifier", (process as any)._startProfilerIdleNotifier);
logHelper("_stopProfilerIdleNotifier", (process as any)._stopProfilerIdleNotifier);
logHelper("reallyExit", (process as any).reallyExit);
logHelper("_fatalException", (process as any)._fatalException);
logHelper("_tickCallback", (process as any)._tickCallback);
logHelper("_getActiveHandles", (process as any)._getActiveHandles);
logHelper("_getActiveRequests", (process as any)._getActiveRequests);
logHelper("openStdin", (process as any).openStdin);
logHelper("_kill", (process as any)._kill);

console.log(
  "active arrays:",
  Array.isArray((process as any)._getActiveHandles()),
  Array.isArray((process as any)._getActiveRequests()),
);
console.log(
  "bootstrap state:",
  Object.prototype.propertyIsEnumerable.call(process, "_preload_modules"),
  Array.isArray((process as any)._preload_modules),
  Object.prototype.propertyIsEnumerable.call(process, "_eval"),
  typeof (process as any)._eval === "undefined" || typeof (process as any)._eval === "string",
  Object.prototype.propertyIsEnumerable.call(process, "_exiting"),
  typeof (process as any)._exiting,
);
console.log(
  "event backing:",
  Object.prototype.propertyIsEnumerable.call(process, "_events"),
  typeof (process as any)._events,
  Object.prototype.propertyIsEnumerable.call(process, "_eventsCount"),
  typeof (process as any)._eventsCount,
  Object.prototype.propertyIsEnumerable.call(process, "_maxListeners"),
  typeof (process as any)._maxListeners,
  Object.prototype.propertyIsEnumerable.call(process, "domain"),
  typeof (process as any).domain,
);
