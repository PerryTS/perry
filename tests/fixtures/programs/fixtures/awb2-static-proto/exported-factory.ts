import { Base } from './base.ts';
export function make() {
  class Exported extends Base { own() { return this.value + 6; } run() { return this.own(); } }
  return Exported;
}
export const First = make();
