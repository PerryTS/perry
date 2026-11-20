// Function constructors keep sloppy semantics even under a module oracle.
const mapped = new Function("a", "b", `
    a = 7;
    const first = arguments;
    console.log('mapped', first[0], first[1], first === arguments);
    first[1] = 8;
    console.log('write', b);
    delete first[0];
    a = 9;
    console.log('unmap', 0 in first, a);
    return () => first === arguments && arguments[1] === b;
`);
console.log('capture', mapped(1, 2)());
const replace = new Function("a", `
    arguments = { length: 1, 0: 37 };
    return arguments[0] + arguments.length;
`);
console.log('replace', replace(1));
const retain = new Function("a", `
    const f = () => arguments;
    a = 10;
    return f;
`);
const retained = retain(2);
console.log('retained', retained()[0], retained() === retained());
const duplicate = new Function("a", "a", `a = 11; return arguments;`);
const dup = duplicate(3, 4);
console.log('duplicate', dup[0], dup[1]);
