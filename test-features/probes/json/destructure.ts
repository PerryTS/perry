const { a, nested: { b } } = JSON.parse('{"a":1,"nested":{"b":2}}');

console.log(`json:${a + b}`);
