// The case that found the bug: `new Array(3e6).fill(0)` and then 3e6 object stores, or
// `.map` into objects with a nested array. fill wrote all three million slots over the
// small store `new Array(n)` backs a length above 1,000,000 with, which on wasm32 ran
// off the end of a 14.6 MB linear memory.
function t(name, f) {
  let r;
  try {
    r = f();
  } catch (e) {
    r = 'threw ' + (e && e.constructor && e.constructor.name);
  }
  console.log(name + ' ' + r);
}
t('fill 3e6', () => {
  const a = new Array(3e6).fill(0);
  let sum = 0;
  for (let i = 0; i < a.length; i += 1000) sum += a[i];
  return a.length + ' ' + sum + ' ' + a[0] + ' ' + a[2999999];
});
t('3e6 object stores', () => {
  const a = new Array(3e6).fill(0);
  for (let i = 0; i < a.length; i++) a[i] = { i };
  let sum = 0;
  for (let i = 0; i < a.length; i += 1000) sum += a[i].i;
  return a.length + ' ' + sum + ' ' + a[0].i + ' ' + a[2999999].i;
});
t('3e6 map to objects with a nested array', () => {
  const a = new Array(3e6).fill(0).map((_, i) => ({ i, s: [i] }));
  let sum = 0;
  for (let i = 0; i < a.length; i += 1000) sum += a[i].s[0];
  return a.length + ' ' + sum + ' ' + a[0].i + ' ' + a[2999999].s[0];
});
