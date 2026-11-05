export class AWDForeign {
  value: number;
  constructor(value: number) { this.value = value; }
  external(x: number) { return this.value + x; }
}
