// RegExp literals born INLINE around the site's matcher data (the data
// address is read from the site's registered root word at every birth) and
// `re.test(s)` at a fused method site that calls the builtin directly and
// searches the string in place, holding no handle. Evacuating minors move
// the strings, the newborn RegExps and the matcher data between and around
// those operations; every answer is checked against a known-correct value,
// so a stale address is observable rather than latent.
function churn(n: number): number {
  const bits: any[] = [];
  for (let i = 0; i < n; i++) {
    bits.push({ i: i, s: "x" + i, pad: [i, i + 1] });
  }
  return bits.length;
}

function digits(): RegExp {
  return /^tag-([0-9]+)$/;
}
function sticky(): RegExp {
  return /a/y;
}
function run(re: any, s: any): any {
  return re.test(s);
}

function main(): number {
  let bad = 0;
  const kept: RegExp[] = [];
  for (let r = 0; r < 60; r++) {
    const re = digits();
    churn(40);
    const yes = "tag-" + r;
    const no = "nope-" + r;
    churn(40);
    if (!run(re, yes)) bad++;
    if (run(re, no)) bad++;
    if (re.lastIndex !== 0) bad++;
    kept.push(re);
    const y = sticky();
    churn(20);
    if (!run(y, "aab")) bad++;
    if (y.lastIndex !== 1) bad++;
    if (!run(y, "aab")) bad++;
    if (run(y, "aab")) bad++;
    if (y.lastIndex !== 0) bad++;
  }
  // Every kept instance is distinct and still searches after the moves.
  for (let i = 0; i < kept.length; i++) {
    if (!run(kept[i], "tag-" + i)) bad++;
    if (i > 0 && kept[i] === kept[i - 1]) bad++;
  }
  return bad;
}

console.log("bad", main());
