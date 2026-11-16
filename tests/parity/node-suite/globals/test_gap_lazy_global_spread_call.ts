// A global function that a program only calls through a spread argument list must exist:
// a spread call reads the binding's value from the global object.
const radix: [string, number] = ["ff", 16];
console.log("parseInt", parseInt(...radix));
console.log("parseFloat", parseFloat(...(["2.5x"] as [string])));
console.log("isNaN", isNaN(...([NaN] as [number])), isFinite(...([1] as [number])));
console.log("uri", encodeURIComponent(...(["a b&c"] as [string])), decodeURI(...(["%41"] as [string])));
console.log("escape", escape(...(["a b"] as [string])), unescape(...(["%41"] as [string])));
console.log("b64", btoa(...(["hi"] as [string])), atob(...(["aGk="] as [string])));
const clone = structuredClone(...([{ a: [1, 2] }] as [object]));
console.log("clone", JSON.stringify(clone));
queueMicrotask(...([() => console.log("microtask")] as [() => void]));
setTimeout(...([() => console.log("timer")] as [() => void]));
