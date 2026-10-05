// recur2: generic class specializations must not reuse an object-literal
// class ID from another module. That collision made the shape claim that
// Recursor instances inherit Object.prototype and dropped recur on clones.
import { literals } from "./_helpers/recur_class_id_literals.ts";
class Recursor<T> {
  value: T;
  constructor(value: T) { this.value = value; }
  recur(n: number): number { return Number(this.value) + n; }
}
class Other { recur(n: number): number { return 100 + n; } }
const original = new Recursor<number>(5);
const clone = Object.create(Object.getPrototypeOf(original), Object.getOwnPropertyDescriptors(original));
console.log(literals.length);
console.log(Object.getPrototypeOf(original) === Recursor.prototype);
console.log(Object.getPrototypeOf(original) === Object.prototype);
function invoke(value: any): number { return value.recur(7); }
console.log(invoke(new Other()));
console.log(invoke(clone));
