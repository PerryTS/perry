class CoreFirst {}
const core = [CoreFirst];
console.log(core.map(c => c.name).join(","));
const target = new EventTarget();
const seen: string[] = [];
target.addEventListener("tick", (event) => {
  seen.push(event.type);
  console.log(event.type, event instanceof Event, event.target === target);
});
const dispatched = target.dispatchEvent(new Event("tick"));
console.log(dispatched, seen.join(","));
console.log(Object.getPrototypeOf(CustomEvent) === Event);
console.log(Object.getPrototypeOf(MessageEvent) === Event);
console.log(new CustomEvent("tick", { detail: 7 }) instanceof Event);
