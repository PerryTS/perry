import { BASE } from "./inl_dep.ts";
console.log("inl_cls init");
export class Adder {
  add(x: number): number {
    return x + BASE;
  }
}
