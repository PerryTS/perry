const a=[1,2,3]; a.foo="f"; const L=[a];
console.log(L[0].foo, L[0].length, typeof L[0].map);
const p={bar:"b", foo:"pf"}; Object.setPrototypeOf(a,p); console.log(L[0].foo, L[0].bar, L[0].length, typeof L[0].map);
delete a.foo; console.log(L[0].foo, L[0].length);
a.length=1; console.log(L[0].length, L[0].bar);
