import { parse } from "my-bindings";
const buf = await Bun.file("input.pdf").bytes();
console.log(parse(buf));
