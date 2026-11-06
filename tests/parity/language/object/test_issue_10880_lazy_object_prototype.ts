// Do not name Object.prototype before these reads: that used to materialize
// the intrinsic and hide the missing implicit prototype on plain objects.
const own: any = { a: 1 };
console.log(typeof own.toString, "toString" in own);
console.log(typeof own.hasOwnProperty, "hasOwnProperty" in own);
console.log(typeof own.valueOf, "valueOf" in own);

const chain: any = Object.create(Object.create({ x: 1 }));
console.log(typeof chain.toString, "toString" in chain, chain.x);

const nullProto: any = Object.create(null);
console.log(typeof nullProto.toString, "toString" in nullProto);

const shadowed: any = { toString: undefined };
console.log(typeof shadowed.toString, "toString" in shadowed);

// Direct call dispatch already worked; keep it last so it cannot initialize
// Object.prototype before the value-read and `in` assertions above.
console.log(own.toString());
