// The start and end of fill are coerced inside the call, and the coercion runs
// user code (`valueOf`, `Symbol.toPrimitive`) that can rebind the variable
// holding the receiver, or grow, shrink or extend the receiver itself. The call
// fills the array the receiver named when it began, the variable keeps whatever
// the callback assigned, and a grown pre-sized array (over one million elements)
// whose variable was left alone gets the grown head back. fill reads the length
// before the coercions, so the range is clamped against the original length.
function digest(a) {
  let sum = 0;
  let holes = 0;
  for (let i = 0; i < a.length; i += 997) {
    if (i in a) sum += typeof a[i] === 'number' ? a[i] : 0;
    else holes++;
  }
  return `len=${a.length} first=${a[0]} last=${a[a.length - 1]} sum=${sum} holes=${holes}`;
}
function show(a) {
  return a.length > 100 ? digest(a) : JSON.stringify(a);
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

// A callback that reassigns the variable, through each coercion hook.
t('small valueOf on start', () => {
  let a = [1, 2];
  const old = a;
  const r = a.fill(7, { valueOf() { a = [9]; return 0; } });
  return show(a) + ' ' + show(old) + ' ' + (r === old) + ' ' + (r === a);
});
t('small valueOf on end', () => {
  let a = [1, 2, 3];
  const old = a;
  const r = a.fill(7, 1, { valueOf() { a = [9]; return 3; } });
  return show(a) + ' ' + show(old) + ' ' + (r === old);
});
t('small toPrimitive on start', () => {
  let a = [1, 2, 3];
  const old = a;
  const r = a.fill(7, { [Symbol.toPrimitive]() { a = [9]; return 1; } });
  return show(a) + ' ' + show(old) + ' ' + (r === old);
});
t('small toPrimitive on end', () => {
  let a = [1, 2, 3];
  const old = a;
  const r = a.fill(7, 0, { [Symbol.toPrimitive](hint) { a = [9]; return hint === 'number' ? 2 : 0; } });
  return show(a) + ' ' + show(old) + ' ' + (r === old);
});
t('small both coercions reassign', () => {
  let a = [1, 2, 3];
  const old = a;
  const first = [4];
  a.fill(7, { valueOf() { a = first; return 0; } }, { valueOf() { a = [5]; return 2; } });
  return show(a) + ' ' + show(old) + ' ' + show(first);
});
t('small callback rebinds to the same array', () => {
  let a = [1, 2];
  const r = a.fill(7, { valueOf() { a = a; return 0; } });
  return show(a) + ' ' + (r === a);
});
t('small callback leaves the variable alone', () => {
  let a = [1, 2, 3];
  let calls = 0;
  const r = a.fill(7, { valueOf() { calls++; return 1; } }, { valueOf() { calls++; return 3; } });
  return calls + ' ' + show(a) + ' ' + (r === a);
});
t('small callback throws, variable untouched', () => {
  let a = [1, 2, 3];
  const old = a;
  try {
    a.fill(7, { valueOf() { a = [9]; throw new RangeError('stop'); } });
  } catch (e) {
    return e.message + ' ' + show(a) + ' ' + show(old);
  }
});

// A pre-sized array past the dense threshold: the call grows it and replaces its head.
t('presized valueOf on start reassigns', () => {
  let a = new Array(N);
  const old = a;
  const r = a.fill(7, { valueOf() { a = [9]; return 100; } });
  return show(a) + ' | ' + digest(old) + ' ' + (r === old) + ' ' + (r === a);
});
t('presized valueOf on end reassigns', () => {
  let a = new Array(N);
  const old = a;
  a.fill(6, 5, { valueOf() { a = [9]; return N - 5; } });
  return show(a) + ' | ' + digest(old);
});
t('presized toPrimitive on start reassigns', () => {
  let a = new Array(N);
  const old = a;
  a.fill(5, { [Symbol.toPrimitive]() { a = [9]; return 0; } });
  return show(a) + ' | ' + digest(old);
});
t('presized toPrimitive on end reassigns', () => {
  let a = new Array(N);
  const old = a;
  a.fill(4, 0, { [Symbol.toPrimitive]() { a = [9]; return N; } });
  return show(a) + ' | ' + digest(old);
});
t('presized callback rebinds to another presized array', () => {
  let a = new Array(N);
  const other = new Array(N + 5);
  const old = a;
  a.fill(3, { valueOf() { a = other; return 0; } });
  return digest(a) + ' | ' + digest(old) + ' ' + (a === other);
});
t('presized callback rebinds to the same array', () => {
  let a = new Array(N);
  const r = a.fill(2, { valueOf() { a = a; return 0; } });
  a.push(1);
  return digest(a) + ' ' + (r === a);
});
t('presized callback leaves the variable alone', () => {
  let a = new Array(N);
  const r = a.fill(8, { valueOf() { return 10; } }, { valueOf() { return N - 10; } });
  a.push(1);
  return digest(a) + ' ' + (r === a);
});
t('presized callback throws, variable untouched', () => {
  let a = new Array(N);
  const old = a;
  try {
    a.fill(7, { valueOf() { a = [9]; throw new RangeError('stop'); } });
  } catch (e) {
    return e.message + ' ' + show(a) + ' ' + digest(old);
  }
});

// The variable lives somewhere other than a plain local slot.
let moduleArray = new Array(N);
t('presized module global reassigned by the callback', () => {
  const old = moduleArray;
  moduleArray.fill(3, { valueOf() { moduleArray = [9]; return 0; } });
  return show(moduleArray) + ' | ' + digest(old);
});
t('presized module global left alone by the callback', () => {
  moduleArray = new Array(N);
  moduleArray.fill(4, { valueOf() { return 0; } });
  return digest(moduleArray);
});
t('presized closure capture reassigned by the callback', () => {
  let a = new Array(N);
  const read = () => a;
  const old = a;
  a.fill(3, { valueOf() { a = [9]; return 0; } });
  return show(read()) + ' ' + show(a) + ' | ' + digest(old);
});
t('presized closure capture left alone by the callback', () => {
  let a = new Array(N);
  const read = () => a;
  a.fill(3, { valueOf() { return 0; } });
  return digest(read()) + ' ' + (read() === a);
});
t('small closure capture reassigned by the callback', () => {
  let a = [1, 2];
  const read = () => a;
  const old = a;
  a.fill(3, { valueOf() { a = [9]; return 0; } });
  return show(read()) + ' ' + show(a) + ' ' + show(old);
});

// The callback changes the receiver itself. The length is read first.
t('small callback pushes', () => {
  let a = [1, 2, 3];
  const r = a.fill(0, { valueOf() { a.push(4); return 0; } });
  return show(a) + ' ' + (r === a);
});
t('small callback shrinks the receiver', () => {
  let a = [1, 2, 3];
  const r = a.fill(0, { valueOf() { a.length = 0; return 0; } });
  return show(a) + ' ' + (r === a);
});
t('small callback shrinks the receiver, range', () => {
  let a = [1, 2, 3, 4];
  a.fill(0, 1, { valueOf() { a.length = 2; return 4; } });
  return show(a);
});
t('small callback grows the receiver past its capacity', () => {
  let a = [1, 2];
  const old = a;
  const r = a.fill(0, { valueOf() { for (let i = 0; i < 5000; i++) a.push(i); return 0; } });
  return digest(a) + ' ' + (r === a) + ' ' + (old === a);
});
t('presized callback pushes', () => {
  let a = new Array(N);
  const r = a.fill(1, { valueOf() { a.push(5); return 0; } });
  return digest(a) + ' ' + (r === a);
});
t('presized callback shrinks the receiver', () => {
  let a = new Array(N);
  a.fill(1, 0, { valueOf() { a.length = 10; return 20; } });
  return show(a);
});
t('start is coerced before end', () => {
  const log = [];
  const a = [1, 2, 3];
  a.fill(0, { valueOf() { log.push('start'); return 0; } }, { valueOf() { log.push('end'); return 3; } });
  return log.join(',') + ' ' + show(a);
});

// A receiver that is not a variable: no write-back, and the callback still runs.
t('literal receiver, callback allocates', () => {
  const r = [1, 2, 3].fill(5, { valueOf() { return [1, 2, 3].length - 2; } });
  return show(r);
});
t('property receiver, callback reassigns the holder', () => {
  const h = { a: [1, 2, 3] };
  const old = h.a;
  const r = h.a.fill(5, { valueOf() { h.a = [9]; return 1; } });
  return show(h.a) + ' ' + show(old) + ' ' + (r === old);
});
