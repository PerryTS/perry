import { isatty } from "node:tty";

// Deno's tty suite imports `isatty` directly and calls it; Node supports the
// captured function reference as a normal callable.
console.log("named invalid fd false:", isatty(1234567) === false);
console.log("named negative fd false:", isatty(-1) === false);
console.log("named fractional fd false:", isatty(0.5) === false);
console.log("named string fd false:", isatty("abc" as any) === false);
console.log("named nullish fd false:", isatty(null as any) === false && isatty(undefined as any) === false);
