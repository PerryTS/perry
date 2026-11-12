// S0 `restlane` (I32 at rest, PR C): `Acc.v` holds Numbers past int32
// (2**31 + k), so its field rep is F64. Under PERRY_FACTS_SABOTAGE=restlane
// the lane admission takes an F64 field too, so `v` becomes an I32 lane. The
// witness at the proven lane store must report the first value that is no
// int32.
class Acc {
  v = 0;
}
function read(x: Acc): number {
  return x.v % 1000;
}
const a = new Acc();
let s = 0;
for (let i = 0; i < 10; i++) {
  a.v = 2147483648 + i;
  s += read(a);
}
console.log(s);
