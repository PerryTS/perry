import { Box, twice } from "./exp_star/js_top.js";
const b = new Box(21);
b.v = twice(b.v);
console.log("js-spec", b.v, b.get());
