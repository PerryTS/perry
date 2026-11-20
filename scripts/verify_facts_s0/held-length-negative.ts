// Held lengths: `Bag.sum` walks `xs`, an alias of the Array the field
// `items` holds, and calls `grow`, which pushes onto that Array through the
// field. A field holds the Array, so the facts give `xs` no stable length
// and the loop reads `xs.length` at every step. Under
// PERRY_FACTS_SABOTAGE=heldlen every Array local of a plain function is
// claimed to have one: the loop would hold the length 3 and stop early. The
// check-mode witness at the held length's first read after the push must
// report the changed length ("held length").
class Bag {
  items: number[] = [1, 2, 3];
  grow(): void {
    if (this.items.length < 6) this.items.push(this.items.length + 1);
  }
  sum(): number {
    const xs = this.items;
    let s = 0;
    for (let i = 0; i < xs.length; i++) {
      s += xs[i] ?? 0;
      this.grow();
    }
    return s;
  }
}
const bag = new Bag();
console.log(bag.sum(), bag.items.length);
