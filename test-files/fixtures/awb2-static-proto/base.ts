export class Base {
  value: number;
  constructor() { this.value = 3; }
  bump() { return this.value + 1; }
  static inherited() { return 7; }
}
export class Other { bump() { return 40; } }
export let Reassigned = Base;
export function replaceParent() { Reassigned = Other as any; }
