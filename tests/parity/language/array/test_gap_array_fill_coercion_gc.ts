// fill's start and end coercions run user code. Here that code allocates enough
// to run evacuating collections while the call holds the receiver, the value to
// store and the end operand only in its own locals: all three must come back
// from the collections at their new addresses, and a variable the callback did
// not touch must still end up holding the (possibly grown) receiver.
function churn(n) {
  const keep = [];
  for (let i = 0; i < n; i++) {
    const o = { i, s: 'item-' + i };
    if (i % 1000 === 0) keep.push(o);
  }
  return keep.length;
}
function digest(a) {
  let sum = 0;
  let holes = 0;
  for (let i = 0; i < a.length; i += 997) {
    if (i in a) sum += typeof a[i] === 'number' ? a[i] : 0;
    else holes++;
  }
  return `len=${a.length} first=${a[0]} last=${a[a.length - 1]} sum=${sum} holes=${holes}`;
}
function t(name, f) {
  let r;
  try {
    r = f();
  } catch (e) {
    r = 'threw ' + (e && e.constructor && e.constructor.name) + ' ' + (e && e.message);
  }
  console.log(name + ' ' + r);
}
const N = 1200000;

t('value object survives a collecting start', () => {
  const a = [1, 2, 3];
  a.fill({ tag: 'v', n: 42 }, { valueOf() { churn(300000); return 1; } });
  return a.map((x) => (typeof x === 'object' ? x.tag + x.n : x)).join(',') + ' ' + (a[1] === a[2]);
});
t('value string survives a collecting end', () => {
  const a = [1, 2, 3];
  const n = 7;
  a.fill('v' + n + 'x', 0, { valueOf() { churn(300000); return 2; } });
  return JSON.stringify(a);
});
t('end operand survives a collecting start', () => {
  const a = [1, 2, 3, 4];
  a.fill(0, { valueOf() { churn(300000); return 1; } }, { valueOf() { return 3; } });
  return JSON.stringify(a);
});
t('literal receiver survives collecting coercions', () => {
  const r = [1, 2, 3].fill({ k: 'lit' }, { valueOf() { churn(300000); return 1; } }, { valueOf() { churn(300000); return 3; } });
  return r.length + ' ' + r[0] + ' ' + r[1].k + ' ' + r[2].k;
});
t('variable keeps the receiver after collecting coercions', () => {
  let a = [1, 2, 3];
  const r = a.fill(9, { valueOf() { churn(300000); return 0; } }, { valueOf() { churn(300000); return 3; } });
  return JSON.stringify(a) + ' ' + (r === a);
});
t('callback reassigns the variable after collecting', () => {
  let a = [1, 2, 3];
  const old = a;
  a.fill({ k: 'v' }, { valueOf() { churn(300000); a = [9]; return 0; } });
  return JSON.stringify(a) + ' ' + old.map((x) => x.k).join(',');
});
t('presized variable gets the grown head after collecting coercions', () => {
  let a = new Array(N);
  const r = a.fill(6, { valueOf() { churn(300000); return 0; } }, { valueOf() { churn(300000); return N; } });
  a.push(1);
  return digest(a) + ' ' + (r === a);
});
t('presized callback reassigns the variable after collecting', () => {
  let a = new Array(N);
  const old = a;
  a.fill(5, { valueOf() { churn(300000); a = [9]; return 0; } });
  return JSON.stringify(a) + ' | ' + digest(old);
});
t('presized heap value after collecting coercions', () => {
  let a = new Array(N);
  a.fill({ id: 'p' }, 10, { valueOf() { churn(300000); return N - 10; } });
  return a.length + ' ' + (10 in a) + ' ' + a[10].id + ' ' + (a[10] === a[N - 11]) + ' ' + (9 in a);
});
t('presized receiver shrunk by an allocating callback', () => {
  let a = new Array(N);
  a.fill(1, 0, { valueOf() { churn(300000); a.length = 10; return 20; } });
  return JSON.stringify(a);
});
