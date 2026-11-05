// process supports the full EventEmitter listener-management surface.
const fn = () => {};
const fn2 = () => {};
process.on("evt-a", fn);
process.addListener("evt-a", fn2);
console.log("listenerCount:", process.listenerCount("evt-a"));
console.log("listenerCount fn:", process.listenerCount("evt-a", fn));
console.log("listeners is array:", Array.isArray(process.listeners("evt-a")));
console.log("rawListeners length:", process.rawListeners("evt-a").length);
console.log("eventNames includes:", process.eventNames().includes("evt-a"));
process.removeListener("evt-a", fn);
console.log("after remove:", process.listenerCount("evt-a"));
process.off("evt-a", fn2);
console.log("after off:", process.listenerCount("evt-a"));
process.on("evt-b", fn);
process.removeAllListeners("evt-b");
console.log("after removeAll one:", process.listenerCount("evt-b"));
process.on("evt-c", fn);
process.removeAllListeners();
console.log("after removeAll all:", process.listenerCount("evt-c"));

function onceFn() {}
process.once("evt-once-raw", onceFn);
const raw = process.rawListeners("evt-once-raw");
const listeners = process.listeners("evt-once-raw");
console.log("raw once is wrapper:", raw[0] !== onceFn);
console.log("raw once listener:", raw[0].listener === onceFn);
console.log("listeners unwrap:", listeners[0] === onceFn);
process.removeAllListeners("evt-once-raw");
