// Issue #1021 follow-up: an async arrow inside a class method captures
// `this` from the enclosing method scope. After phase 2 the arrow body
// becomes a state machine; the body's `this` references must still resolve
// to the instance through the synthesized step closure.

class Counter {
  value: number = 10;

  async bump(): Promise<number> {
    const cb = async (): Promise<number> => {
      await Promise.resolve();
      this.value += 1;
      return this.value;
    };
    const a = await cb();
    const b = await cb();
    return a + b;
  }
}

(async () => {
  const c = new Counter();
  const sum = await c.bump();
  // Expect: 11 + 12 = 23
  console.log("sum=" + sum);
})();
