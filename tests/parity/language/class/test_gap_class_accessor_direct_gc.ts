// The receiver and setter RHS stay live through collections inside the body.
class Entry {
  data: any = 1;
  get value() {
    const saved = [this];
    if (typeof (globalThis as any).gc === 'function') (globalThis as any).gc();
    return saved[0].data;
  }
  set value(next: any) {
    const saved = [this, next];
    if (typeof (globalThis as any).gc === 'function') (globalThis as any).gc();
    saved[0].data = saved[1];
  }
}
function exercise(entry: Entry) {
  let sum = 0;
  for (let i = 0; i < 30; i++) {
    entry.value = { amount: i };
    sum += entry.value.amount;
  }
  return sum;
}
console.log(exercise(new Entry()));
