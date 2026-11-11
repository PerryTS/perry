// S0 `restlane` (I32 at rest, PR C): `Box.v` holds Numbers past int32
// (2**31 + k), so its field rep is F64. Under PERRY_FACTS_SABOTAGE=restlane
// the lane admission takes an F64 field too, so the object-literal class gets
// an I32 lane at `v`. The witness at the birth's store must report the first
// value that is no int32.
interface Box {
  v: number;
}
function make(k: number): Box {
  return { v: 2147483648 + k };
}
let s = 0;
for (let i = 0; i < 10; i++) {
  const b = make(i);
  s += b.v % 1000;
}
console.log(s);
