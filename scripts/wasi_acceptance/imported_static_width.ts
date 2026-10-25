// An importing module declared a cross-module static method with six double
// parameters whatever the producer declared. A wasm32 object records each
// function's type, so wasm-ld rejected the symbol with two signatures; the
// program must link, and arguments past the declared width are ignored.
import { Calc } from "./_helpers/static_width_lib.ts";
const trace: string[] = [];
const mark = (label: string) => { trace.push(label); return label; };
console.log(Calc.zero(), Calc.twice(21), Calc.add(1, 2));
console.log(Calc.add(1, 2, mark("extra")), Calc.twice(4, mark("again")));
console.log(trace.join(","));
