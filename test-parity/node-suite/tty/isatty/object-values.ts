import * as tty from "node:tty";

console.log("plain object false:", tty.isatty({} as any) === false);
console.log("array false:", tty.isatty([] as any) === false);
console.log("function false:", tty.isatty((() => {}) as any) === false);
console.log("date false:", tty.isatty(new Date(0) as any) === false);
