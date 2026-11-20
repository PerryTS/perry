// family_reach: the typed-array family is observed without any byte cell
// being born (a constructor read by name). Under
// PERRY_FACTS_SABOTAGE=famreach:typedarray the witness at the read of a
// global by name must report the observation ("family").
const names: string[] = [];
for (const C of [Uint8Array, Float64Array] as any[]) names.push(C.name);
console.log(names.join(","));
