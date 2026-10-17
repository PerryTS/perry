// Gap test for #12698: an inherited read on a long Object.create chain
// answered `undefined` past ~128 hops (the recursive resolver's depth cap),
// while `"k" in obj` stayed true. The resolver now skips every prototype whose
// shape proves the key absent, so only possible holders count toward the
// recursion. Byte-identical to node.
for (const n of [10, 50, 100, 128, 129, 200, 255, 256, 257, 270, 1000, 5000]) {
  const c: any[] = [Object.create(null)];
  for (let i = 1; i < n; i++) c.push(Object.create(c[i - 1]));
  c[Math.min(6, n - 1)].k = 7;
  const bottom = c[n - 1];
  console.log(n, bottom.k, bottom.missing, "k" in bottom, "missing" in bottom);
}
{
  const chain: any[] = [Object.create(null)];
  for (let i = 1; i < 270; i++) chain.push(Object.create(chain[i - 1]));
  chain[6].k = 7;
  console.log(chain[269].k, chain[269].missing, "k" in chain[269]);
  chain[3].j = 1;
  console.log(chain[269].j, chain[5].k);
  chain[6].k = 8;
  console.log(chain[269].k);
  delete chain[6].k;
  console.log(chain[269].k, "k" in chain[269]);
  Object.defineProperty(chain[2], "g", { get() { return this === chain[269] ? "recv" : "other"; } });
  console.log(chain[269].g);
  chain[200].k = "near";
  console.log(chain[269].k);
}
{
  // Default-prototype chains end at Object.prototype.
  let o: any = {};
  for (let i = 0; i < 400; i++) o = Object.create(o);
  console.log(typeof o.hasOwnProperty, o.toString(), o.nope);
}
{
  // Mixed holders: each level adds a different key.
  const c: any[] = [{}];
  for (let i = 1; i < 300; i++) {
    const o = Object.create(c[i - 1]);
    if (i % 50 === 0) o["k" + i] = i;
    c.push(o);
  }
  const b = c[299];
  console.log(b.k50, b.k100, b.k250, b.k299, b.k1);
}
