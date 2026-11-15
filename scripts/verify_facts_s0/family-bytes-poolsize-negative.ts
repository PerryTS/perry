// family_reach: `Buffer.poolSize` observes the Buffer constructor and births
// nothing. Under PERRY_FACTS_SABOTAGE=famreach:typedarray the witness at the
// read of a global by name must report it ("family").
const B: any = Buffer;
console.log(typeof B.poolSize);
