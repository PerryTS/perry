const events: string[] = [];

const timeout: any = setTimeout(function (this: any) {
  events.push("timeout same:" + (this === timeout));
  events.push("timeout primitive:" + (+this === +timeout));
}, 1);

await new Promise<void>((resolve) => setTimeout(() => resolve(), 20));
console.log(events.join("\n"));
