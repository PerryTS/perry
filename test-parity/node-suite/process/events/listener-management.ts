// process supports the full EventEmitter listener-management surface.
const fn = () => {};
process.on("evt-a", fn);
console.log("listenerCount:", process.listenerCount("evt-a"));
console.log("listeners is array:", Array.isArray(process.listeners("evt-a")));
console.log("eventNames includes:", process.eventNames().includes("evt-a"));
process.removeListener("evt-a", fn);
console.log("after remove:", process.listenerCount("evt-a"));
