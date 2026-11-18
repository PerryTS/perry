class CoreFirst {}
const core = [CoreFirst];
console.log(core.map(c => c.name).join(","));
const Controller = AbortController;
const controller = new Controller();
const signal = controller.signal;
console.log(signal.aborted, signal instanceof AbortSignal, signal instanceof EventTarget);
controller.abort("stop");
console.log(signal.aborted, signal.reason);
console.log(Object.getPrototypeOf(AbortSignal) === EventTarget);
