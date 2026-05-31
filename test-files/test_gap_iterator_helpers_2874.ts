// #2874: global Iterator + iterator helper methods (TC39 iterator-helpers).

function* gen() {
  yield 1;
  yield 2;
  yield 3;
}

// typeof surface
console.log(typeof Iterator);
console.log(typeof Iterator.from);

// Iterator.from + toArray
console.log(Iterator.from([1, 2, 3]).toArray());

// map over a generator, spread the result
console.log([...gen().map((x) => x * 2)]);

// filter
console.log(Iterator.from([1, 2, 3]).filter((x) => x > 1).toArray());

// chained map + filter + spread
console.log([...Iterator.from([1, 2, 3, 4]).map((x) => x * 2).filter((x) => x > 4)]);

// take
console.log(Iterator.from([1, 2, 3, 4, 5]).take(2).toArray());

// drop
console.log(Iterator.from([1, 2, 3, 4, 5]).drop(1).toArray());

// take after a lazy chain over a generator
console.log([...gen().filter((x) => x >= 2).take(1)]);

// reduce
console.log(Iterator.from([1, 2, 3]).reduce((a, b) => a + b, 0));
console.log(Iterator.from([1, 2, 3, 4]).reduce((a, b) => a + b));

// some / every / find
console.log(Iterator.from([1, 2, 3]).some((x) => x === 2));
console.log(Iterator.from([1, 2, 3]).every((x) => x > 0));
console.log(Iterator.from([1, 2, 3]).find((x) => x > 1));

// flatMap
console.log(Iterator.from([1, 2]).flatMap((x) => [x, x * 10]).toArray());

// forEach
let sum = 0;
Iterator.from([1, 2, 3]).forEach((x) => {
  sum += x;
});
console.log(sum);
