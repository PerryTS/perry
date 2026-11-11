// 2000 pushes in a loop. A rule runs once per token, not once per runtime
// push, so an append raises the length's upper bound to the largest length
// (`solve::arr_len`). Under PERRY_FACTS_SABOTAGE=arrappend it raises it by
// one per rule: the facts claim `a.length` stays tiny, so `i < a.length`
// bounds `i` and `k = i * 3000000` is claimed an int32. The int32 witness
// must report `k` once it passes 2^31.
const a: number[] = [];
for (let i = 0; i < 2000; i++) a.push(i);
function total(): number {
  let s = 0;
  for (let i = 0; i < a.length; i++) {
    const k = i * 3000000;
    s += (a[k & 1] ?? 0) + k;
  }
  return s;
}
console.log(total(), a[1500]);
