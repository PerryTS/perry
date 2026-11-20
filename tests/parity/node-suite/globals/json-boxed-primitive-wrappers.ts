// #3857: JSON.stringify of boxed primitive wrappers serializes the underlying
// primitive, not the (empty) wrapper object.
console.log(JSON.stringify(new String("hi")));
console.log(JSON.stringify(new Number(5)));
console.log(JSON.stringify(new Boolean(true)));
console.log(JSON.stringify({ a: new String("x"), b: new Number(2), c: new Boolean(false) }));
console.log(JSON.stringify([new String("a"), new Number(1), new Boolean(true)]));
console.log(JSON.stringify({ n: new Number(3), s: new String("y") }, null, 2));
