// Refs #1849: a zero-argument numeric method with one direct field update can
// execute against scalar-replaced slots. The second case is the runtime-truth
// guardrail: the annotation is only a hint, so a string passed through `any`
// must retain JavaScript `+` semantics rather than being forced into f64.
class Counter {
  value: number;

  constructor(value: number) {
    this.value = value;
  }

  bump(): number {
    this.value = this.value + 1;
    return this.value;
  }
}

const numeric = new Counter(40);
console.log(numeric.bump(), numeric.bump(), numeric.value);

const dynamic = new Counter("x" as any);
console.log(dynamic.bump(), dynamic.bump(), dynamic.value);
