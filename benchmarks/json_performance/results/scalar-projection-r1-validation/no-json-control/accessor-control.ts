const events: string[] = [];
const descriptor: any = {get value() { events.push("getter"); return 7; }};
console.log(descriptor.value, events.length);
