// A timer delay goes through ToNumber, so a BigInt delay throws a TypeError
// before the timer is scheduled; util.format still prints a BigInt for %d.
import { format } from "node:util";

function show(label: string, run: () => unknown): void {
  try {
    console.log(label, String(run()));
  } catch (e) {
    console.log(label, (e as Error).constructor.name);
  }
}

const one: any = 1n;
show("setTimeout", () => typeof setTimeout(() => console.log("fired"), one));
show("setInterval", () => typeof setInterval(() => console.log("fired"), one));
show("format d", () => format("%d|%i|%f", one, one, one));
