// What a pre-sized array past the 1,000,000 dense threshold does after `fill` has written it:
// the array is a normal array: it reads, grows and shrinks, and a binding that kept the
// pre-fill reference (a closure, a module global, an alias) sees the fill. Companion to
// test_gap_array_fill_presized, split from it to stay inside the harness's 10 s run timeout.
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

t('fill then reads', () => {
  const a = new Array(1000001).fill(3);
  return [a.indexOf(3), a.lastIndexOf(3), a.includes(3), a.includes(4), a.at(-1), a.slice(-2).join()].join(' ');
});
t('fill then push, pop, length', () => {
  const a = new Array(1000001).fill(1);
  a.push(2);
  a.push(3);
  const popped = a.pop();
  return popped + ' ' + digest(a);
});
t('fill then set after fill', () => {
  const a = new Array(1000001).fill(0, 0, 1000000);
  a[5] = 'x';
  a[1000000] = 'y';
  a[1000001] = 'z';
  return a.length + ' ' + a[5] + a[1000000] + a[1000001] + ' ' + a[6];
});
let moduleArray = new Array(1000001);
t('fill module global', () => {
  moduleArray.fill(4);
  return digest(moduleArray);
});
t('fill captured by closure', () => {
  const a = new Array(1000001);
  const f = () => a[1000000] + a[0];
  a.fill(8, 0, 1000001);
  return f();
});
t('fill local then read', () => {
  const a = new Array(1100000);
  a.fill(1, 1, 1099990);
  return a[0] + ' ' + a[1] + ' ' + a[1099989] + ' ' + a[1099990] + ' ' + digest(a);
});
t('fill alias', () => {
  const a = new Array(1000001);
  const b = a;
  a.fill(3, 0, 1000001);
  b[7] = 4;
  return a[7] + ' ' + b[8] + ' ' + (a === b);
});
