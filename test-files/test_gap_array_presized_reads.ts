// Reading a pre-sized array that was never filled: `new Array(n)` above 1,000,000
// elements has a logical length over a small backing store, so every loop that
// walks 0..length has to stop at the store and treat the rest as holes.
function digest(a) {
  let sum = 0;
  let holes = 0;
  let others = 0;
  for (let i = 0; i < a.length; i += 997) {
    if (i in a) {
      if (typeof a[i] === 'number') sum += a[i];
      else others++;
    } else holes++;
  }
  const last = a.length - 1;
  return `len=${a.length} first=${a[0]} last=${a[last]} sum=${sum} holes=${holes} other=${others}`;
}
function t(name, f) {
  let r;
  try {
    r = f();
  } catch (e) {
    r = 'threw ' + (e && e.constructor && e.constructor.name);
  }
  console.log(name + ' ' + r);
}

// Unfilled large arrays read as holes through every route.
t('unfilled reads', () => {
  const a = new Array(1200000);
  let n = 0;
  a.forEach(() => n++);
  return [a.length, n, a.indexOf(undefined), a.includes(undefined), a.map((v) => 1).length, a.filter(() => true).length,
    a.join('').length, a.slice(1000, 1003).length, 5 in a, a.findIndex((v) => v === undefined), a.at(-1)].join(' ');
});
t('unfilled reverse', () => {
  const a = new Array(1200000);
  a.reverse();
  return digest(a);
});
t('unfilled sort', () => {
  const a = new Array(1200000);
  a.sort();
  return digest(a);
});
t('unfilled with a tail value', () => {
  const a = new Array(1200000);
  a[1199999] = 1;
  a[3] = 2;
  const r = a.fill(0, 4, 8);
  return digest(r);
});
t('unfilled iteration', () => {
  const a = new Array(1200000);
  let n = 0;
  for (const v of a) if (v === undefined) n++;
  let m = 0;
  for (const k of a.keys()) m++;
  let e = 0;
  for (const [k, v] of a.entries()) e++;
  return [n, m, e, a.some((v) => v !== undefined), a.every((v) => v === undefined), a.find((v) => v === undefined), a.reduce((x) => x + 1, 0)].join(' ');
});
t('unfilled copies', () => {
  const a = new Array(1200000);
  const s = a.slice(0, 10);
  const c = a.concat([1]);
  const sp = [...a];
  const f = Array.from(a);
  return [s.length, c.length, c[1200000], sp.length, sp[5], f.length, f[1199999], 3 in c, 3 in sp, 3 in f].join(' ');
});
t('unfilled in-place', () => {
  const a = new Array(1200000);
  a.copyWithin(0, 5, 10);
  const c = new Array(1200000);
  c.shift();
  const d = new Array(1200000);
  d.unshift(1);
  const e = new Array(1200000);
  e.length = 1100000;
  const g = new Array(1200000);
  g.length = 1300000;
  return [digest(a), digest(c), digest(d), digest(e), digest(g)].join(' | ');
});
t('unfilled stringify', () => {
  const a = new Array(1200000);
  return JSON.stringify(a).length + ' ' + a.toString().length + ' ' + String(a.flat().length) + ' ' + Object.keys(a).length;
});
