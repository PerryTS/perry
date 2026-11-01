// A literal family that gains a key later names class 0: its objects are told
// by the family descriptor word, and a proven read of an early key must agree.
const L: any = { basic: { u: 1 }, lambert: { u: 2 }, phong: { u: 3 }, standard: { u: 4 }, toon: { u: 5 } };
L.physical = { u: L.standard.u + 10 };
console.log(L.standard.u, L.physical.u, Object.keys(L).length);
