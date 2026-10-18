export class Parent {
  value: number;
  constructor(value: number) { this.value = value; }
  read() { return "parent:" + this.value; }
}
