// family_reach: a typed array is born. Under
// PERRY_FACTS_SABOTAGE=famreach:typedarray the fact says the typed-array
// family is never created or observed; the instrumented runtime's witness at
// the one allocation of every byte cell (`buffer::store::store_alloc`) must
// report the birth ("family").
const a = new Uint8Array(4);
a[1] = 7;
console.log(a.length, a[1]);
