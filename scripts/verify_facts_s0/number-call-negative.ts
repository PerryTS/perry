// `label()` returns a string. Under PERRY_FACTS_SABOTAGE=numbercall the facts
// claim every method call returns a Number: the call-result witness of the
// Number-call proof must report it.
class Box {
  v: number;
  constructor(v: number) {
    this.v = v;
  }
  label(): string {
    return "n" + this.v;
  }
}
const box = new Box(3);
const text = box.label();
console.log(text.length + 1);
