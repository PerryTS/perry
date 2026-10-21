// An argument of fill can rebind the variable that holds the receiver:
// a.fill((a = [9], 0)). The call fills the array `a` named when the call began,
// and the variable keeps the array the argument assigned. fill grows a pre-sized
// array past one million elements, which replaces its head, and the head used to
// be written back into the variable after the arguments ran, rebinding it to the
// filled original. A variable the arguments leave alone still gets the grown head.
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
    r = 'threw ' + (e && e.constructor && e.constructor.name);
  }
  console.log(name + ' ' + r);
}

// Small arrays (no growth): the original is filled, the variable holds the new array.
t('small value arg', () => {
  let a = [1, 2];
  const old = a;
  const r = a.fill((a = [9], 0));
  return show(a) + ' ' + show(old) + ' ' + (r === old) + ' ' + (r === a);
});
t('small start arg', () => {
  let a = [1, 2, 3];
  const old = a;
  const r = a.fill(7, (a = [9], 1));
  return show(a) + ' ' + show(old) + ' ' + (r === old);
});
t('small end arg', () => {
  let a = [1, 2, 3];
  const old = a;
  const r = a.fill(7, 1, (a = [9], 2));
  return show(a) + ' ' + show(old) + ' ' + (r === old);
});
t('small rebound to another array of the same length', () => {
  let a = [1, 2];
  const other = [5, 6];
  const old = a;
  a.fill((a = other, 0));
  return show(a) + ' ' + show(old) + ' ' + (a === other);
});
t('small rebound to itself', () => {
  let a = [1, 2];
  const r = a.fill((a = a, 4));
  return show(a) + ' ' + (r === a);
});
t('small no reassign', () => {
  let a = [1, 2, 3];
  const r = a.fill(0);
  return show(a) + ' ' + (r === a);
});

// Pre-sized arrays past the dense threshold: fill grows the store and replaces the head.
const N = 1200000;
t('presized value arg', () => {
  let a = new Array(N);
  const old = a;
  const r = a.fill((a = [9], 3));
  return show(a) + ' | ' + digest(old) + ' ' + (r === old) + ' ' + (r === a);
});
t('presized range value arg', () => {
  let a = new Array(N);
  const old = a;
  a.fill((a = [9], 4), 10, N - 10);
  return show(a) + ' | ' + digest(old);
});
t('presized start arg', () => {
  let a = new Array(N);
  const old = a;
  a.fill(5, (a = [9], 100));
  return show(a) + ' | ' + digest(old);
});
t('presized end arg', () => {
  let a = new Array(N);
  const old = a;
  a.fill(6, 0, (a = [9], N));
  return show(a) + ' | ' + digest(old);
});
t('presized rebound to another presized array', () => {
  let a = new Array(N);
  const other = new Array(N + 5);
  const old = a;
  a.fill((a = other, 2));
  return digest(a) + ' | ' + digest(old) + ' ' + (a === other);
});
t('presized rebound to itself', () => {
  let a = new Array(N);
  const r = a.fill((a = a, 1));
  return digest(a) + ' ' + (r === a);
});
t('presized no reassign', () => {
  let a = new Array(N);
  const r = a.fill(8);
  a.push(1);
  return digest(a) + ' ' + (r === a);
});
t('presized no reassign range', () => {
  let a = new Array(N);
  a.fill(8, 5, N - 5);
  return digest(a);
});
t('presized argument with side effects that leave the variable alone', () => {
  let a = new Array(N);
  let calls = 0;
  const r = a.fill((calls++, 2), (calls++, 0), (calls++, N));
  return calls + ' ' + digest(a) + ' ' + (r === a);
});

// The variable lives somewhere other than a plain local slot.
let moduleArray = new Array(N);
t('presized module global reassigned by the argument', () => {
  const old = moduleArray;
  moduleArray.fill((moduleArray = [9], 3));
  return show(moduleArray) + ' | ' + digest(old);
});
t('presized module global not reassigned', () => {
  moduleArray = new Array(N);
  moduleArray.fill(4);
  return digest(moduleArray);
});
t('presized captured by a closure, reassigned by the argument', () => {
  let a = new Array(N);
  const read = () => a;
  const old = a;
  a.fill((a = [9], 3));
  return show(read()) + ' ' + show(a) + ' | ' + digest(old);
});
t('presized captured by a closure, not reassigned', () => {
  let a = new Array(N);
  const read = () => a;
  a.fill(3);
  return digest(read()) + ' ' + (read() === a);
});
t('presized reassigned inside a closure called by the argument', () => {
  let a = new Array(N);
  const swap = () => {
    a = [9];
    return 3;
  };
  const old = a;
  a.fill(swap());
  return show(a) + ' | ' + digest(old);
});
t('small reassigned inside a closure called by the argument', () => {
  let a = [1, 2];
  const swap = () => {
    a = [9];
    return 3;
  };
  const old = a;
  a.fill(swap());
  return show(a) + ' ' + show(old);
});
