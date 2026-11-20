export const Ü = 17;
export const $val = 23;
export const _val = 29;
export function grüße(value = "café") { return "grüße " + value; }
export { grüße as salüt };
export function $() { return 31; }
export function u__24_() { return 37; }
export function \u03BB(value = 41) { return value + 1; }

export class Étage {
  value: string;
  constructor(value = "café") { this.value = value; }
  méthode() { return this.value; }
  get résumé() { return "résumé " + this.value; }
  set résumé(value: string) { this.value = value; }
  static déjà() { return "déjà"; }
  static Ü = 43;
}

export class $Étage {
  value: number;
  constructor(value = 67) { this.value = value; }
  méthode() { return this.value; }
}

export default class Über {
  value: string;
  constructor(value = "über") { this.value = value; }
  schön() { return this.value; }
}
