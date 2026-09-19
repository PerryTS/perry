// #10445: a generator method keyed by `[Symbol.iterator]`, whose `for…of`
// iterable is a method call on `this` (`for (const x of this.gen()) …`),
// saw `this === undefined` inside the callee. Root cause:
// `synthesize_symbol_iterator_wrapper` (lower_decl/class_decl.rs) lifts the
// method's body to a top-level generator taking `this` as an explicit
// param, then `replace_this_in_stmts`/`replace_this_in_expr` (analysis.rs)
// rewrites every `Expr::This` in that body to the param. A `for…of` whose
// iterable can't be proven a plain Array/Map/Set lowers to one of
// `GetIterator`/`GetAsyncIterator`/`MapEntries`/`SetValues` wrapping the
// receiver expression (stmt_loops.rs's `lower_stmt_for_of_inner`) -- and
// `replace_this_in_expr` had no arm for any of those wrappers, so a `this`
// buried inside one fell through to the catch-all and was left unreplaced.
// Every consumer that dispatches through the lifted function (spread,
// `for…of`, `Array.from`) hit the same bug identically.

class Bag {
  items = [1, 2];
  *gen() {
    yield* this.items;
  }
  *[Symbol.iterator]() {
    for (const x of this.gen()) yield x; // the repro shape
  }
  *viaLocal() {
    for (const x of this.gen()) yield x; // same body, ordinary name: control
  }
}

// Two-level: the iterator method's for-of iterable is ANOTHER method whose
// OWN for-of iterable is a THIRD method -- this must survive two hops of
// the lifted-generator's this-substitution, not just one.
class TwoLevel {
  items = [10, 20, 30];
  *inner() {
    for (const x of this.items) yield x * 2;
  }
  *middle() {
    for (const x of this.inner()) yield x + 1;
  }
  *[Symbol.iterator]() {
    for (const x of this.middle()) yield x;
  }
}

// Class EXPRESSION (not a declaration) -- the lift/this-rewrite must not be
// keyed off a named-declaration-only path.
const ExprClass = class {
  items = ["a", "b", "c"];
  *gen() {
    yield* this.items;
  }
  *[Symbol.iterator]() {
    for (const x of this.gen()) yield x;
  }
};

// `yield*` delegation alongside a for-of over `this.method()` in the SAME
// generator -- confirms the fix doesn't disturb the already-working
// yield*-over-this.gen() path while also fixing the for-of one.
class Mixed {
  items = [1, 2, 3];
  *gen() {
    yield* this.items;
  }
  *[Symbol.iterator]() {
    yield* this.gen();
    for (const x of this.gen()) yield x * 10;
  }
}

const show = (label: string, f: () => unknown) => {
  try {
    console.log(label, JSON.stringify(f()));
  } catch (e: any) {
    console.log(label, "threw:", e.message);
  }
};

show("spread over *[Symbol.iterator]:", () => [...new Bag()]);
show("for-of over *[Symbol.iterator]:", () => {
  const out: number[] = [];
  for (const x of new Bag()) out.push(x);
  return out;
});
show("Array.from(bag):", () => Array.from(new Bag()));
show("named generator, same body:", () => [...new Bag().viaLocal()]);
show("two-level generator:", () => [...new TwoLevel()]);
show("class expression generator:", () => [...new ExprClass()]);
show("yield* + for-of mixed:", () => [...new Mixed()]);
