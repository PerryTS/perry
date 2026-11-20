class Counter {
  #value = 1;
  static #total = 1;

  next() {
    this.#value += 1;
    return this.#value;
  }

  static next() {
    this.#total += 1;
    return this.#total;
  }
}

console.log(`classes/private-static-fields:${new Counter().next()},${Counter.next()}`);
