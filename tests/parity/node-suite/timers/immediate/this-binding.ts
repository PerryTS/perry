const events: string[] = [];

const immediate: any = setImmediate(function (this: any, a: string) {
  events.push("immediate same:" + (this === immediate));
  events.push("arg:" + a);
}, "x");

await new Promise<void>((resolve) => setTimeout(() => resolve(), 20));
console.log(events.join("\n"));
