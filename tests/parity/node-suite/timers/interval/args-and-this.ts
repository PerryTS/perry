const events: string[] = [];

const interval: any = setInterval(function (this: any, a: string, b: string) {
  events.push("interval same:" + (this === interval));
  events.push("interval primitive:" + (+this === +interval));
  events.push("args:" + a + b);
  clearInterval(interval);
}, 1, "a", "b");

await new Promise<void>((resolve) => setTimeout(() => resolve(), 20));
console.log(events.join("\n"));
