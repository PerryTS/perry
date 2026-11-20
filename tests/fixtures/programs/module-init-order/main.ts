import { lookup, count } from './registry';
// Side-effect import — triggers register-defaults.ts's top-level code.
import './register-defaults';

console.log("count=" + count());
console.log("16=" + lookup(16));
console.log("25=" + lookup(25));
console.log("23=" + lookup(23));
console.log("999=" + lookup(999));
