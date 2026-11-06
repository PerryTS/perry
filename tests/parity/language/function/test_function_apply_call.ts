function add(a: number, b: number) { return a + b; }
function greet(this: { name: string }, prefix: string) { return prefix + ' ' + this.name; }

console.log(add.apply(null, [2, 3]));        // 5
console.log(add.call(null, 2, 3));            // 5
console.log(add.apply(null, [10, 20]));       // 30
console.log(greet.call({ name: 'Bob' }, 'Hi')); // 'Hi Bob'
console.log(greet.apply({ name: 'Sue' }, ['Hello'])); // 'Hello Sue'

// Method on arrow function (no this rebind)
const sq = (x: number) => x * x;
console.log(sq.apply(null, [4]));  // 16
console.log(sq.call(null, 5));     // 25
