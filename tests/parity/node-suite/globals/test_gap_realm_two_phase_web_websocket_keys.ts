class CoreFirst {}
const core = [CoreFirst];
console.log(core.map(c => c.name).join(","));
console.log(Object.getOwnPropertyNames(WebSocket).join(","));
console.log(Object.getOwnPropertyNames(WebSocket.prototype).join(","));
