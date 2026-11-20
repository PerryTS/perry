// A Symbol, a class with a static, and a Map made by one module's init.
export const tag = Symbol("inventory-i1");
export class Registry {
  static count = 1;
  value = 2;
}
export const table = new Map<string, number>([["a", 1], ["b", 2]]);
export const made = new Registry();
