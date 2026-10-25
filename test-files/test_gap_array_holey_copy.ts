// Copying holey arrays and array-likes: Array.from, spread, slice, concat,
// splice, fill and the borrowed `Array.prototype.slice.call(arrayLike)`. A hole
// is preserved where the spec preserves it (slice, concat, splice) and becomes
// `undefined` where it does not (Array.from, spread), and an indexed property
// added to Array.prototype / Object.prototype -- before or after the first
// copy -- must be seen, including a getter, since the fast paths are only
// valid while no such property exists.
function show(a) {
  // No array built here: an accessor on Array.prototype[1] would intercept push.
  let out = a.length + ':[';
  for (let i = 0; i < a.length; i++) out += (i ? ',' : '') + (i in a ? String(a[i]) : '<hole>');
  return out + ']';
}
// Object.keys is exact for small arrays. Perry's Object.keys skips the dense
// indices of an array above 100000 elements (a size cap in its enumeration), so
// a large array is probed through hasOwnProperty at a stride instead.
function ownCount(a) {
  if (a.length <= 1000) return Object.keys(a).length;
  let n = 0;
  for (let i = 0; i < a.length; i += 997) if (Object.prototype.hasOwnProperty.call(a, i)) n++;
  return n;
}
const getterCalls = { length: 0 };

// `fills` is off while an inherited accessor with a setter is installed: node's
// fill calls that setter on a hole, which Perry's fill does not.
function ops(label, fills = true) {
  const holey = [1, , 3, , 5];
  holey.length = 7;
  const arrayLike = { length: 6, 1: 'b', 4: 'e' };
  const empty = { length: 4 };
  const o = (name, f) => {
    let r;
    try {
      r = f();
    } catch (e) {
      r = 'threw ' + (e && e.constructor && e.constructor.name);
    }
    console.log(label + ' ' + name + ' ' + r);
  };
  o('from(arrayLike)', () => show(Array.from(arrayLike)));
  o('from(empty)', () => show(Array.from({ length: 4 })));
  o('from(holey)', () => show(Array.from(holey)));
  o('from(holey,map)', () => show(Array.from(holey, (v, i) => i + ':' + v)));
  o('spread', () => show([...holey]));
  o('slice', () => show(holey.slice()));
  o('slice(1,5)', () => show(holey.slice(1, 5)));
  o('slice.call(arrayLike)', () => show(Array.prototype.slice.call(arrayLike)));
  o('slice.call(empty)', () => show(Array.prototype.slice.call(empty)));
  o('concat self', () => show(holey.concat(holey)));
  o('concat lead', () => show([0].concat(holey, [9])));
  o('splice', () => show([].concat(holey).splice(0, 5)));
  o('fill', () => show(holey.slice().fill(7, 1, 4)));
  if (fills) o('fill all', () => show(new Array(5).fill(0)));
  o('Array.apply', () => show(Array.apply(null, { length: 3 })));
  o('own keys of from', () => ownCount(Array.from(empty)));
  o('own keys of slice', () => ownCount(holey.slice()));
  o('own keys of concat', () => ownCount(holey.concat(holey)));
  const big = new Array(3e5);
  big[5] = 'five';
  o('big from(holey)', () => ownCount(Array.from(big)) + ' ' + Array.from(big).length);
  o('big from(length)', () => ownCount(Array.from({ length: 3e5 })));
  o('big slice', () => ownCount(big.slice()) + ' ' + big.slice().length);
  o('big concat', () => ownCount(big.concat(big)) + ' ' + big.concat(big).length);
  o('big spread', () => ownCount([...big]) + ' ' + [...big].length);
  if (fills) o('big fill', () => {
    const f = new Array(3e5).fill(2);
    let s = 0;
    for (let i = 0; i < f.length; i++) s += f[i] === 2 ? 1 : 0;
    return s + ' ' + ownCount(f);
  });
}

ops('pristine');
ops('pristine again');

Array.prototype[3] = 'ap3';
Object.defineProperty(Array.prototype, 1, {
  configurable: true,
  get() {
    getterCalls.length++;
    return 'getter1';
  },
  set(v) {},
});
ops('array-proto', false);
console.log('getter calls ' + (getterCalls.length > 0));
delete Array.prototype[3];
delete Array.prototype[1];
ops('array-proto removed');

Object.prototype[2] = 'op2';
Object.defineProperty(Object.prototype, 4, {
  configurable: true,
  get() {
    return 'getter4';
  },
  set(v) {},
});
ops('object-proto', false);
delete Object.prototype[2];
delete Object.prototype[4];
ops('object-proto removed');

// A property added after copies have already been made is seen by the next one.
const late = { length: 3 };
console.log('late before ' + show(Array.from(late)));
Object.prototype[1] = 'late1';
console.log('late after ' + show(Array.from(late)));
delete Object.prototype[1];
console.log('late removed ' + show(Array.from(late)));

// Own index keys, a null prototype and a class instance keep their own answers.
console.log('own ' + show(Array.from({ length: 3, 0: 'a', 2: 'c' })));
console.log('null proto ' + show(Array.from(Object.assign(Object.create(null), { length: 3, 1: 'n' }))));
class Box {
  constructor() {
    this.length = 3;
  }
  get 1() {
    return 'boxed';
  }
}
console.log('class ' + show(Array.from(new Box())));
const proto = { 0: 'p0' };
console.log('custom proto ' + show(Array.from(Object.setPrototypeOf({ length: 2 }, proto))));
const getterLength = {
  get length() {
    Object.prototype[0] = 'from-length-getter';
    return 2;
  },
};
console.log('length getter ' + show(Array.from(getterLength)));
delete Object.prototype[0];

// A prototype swapped in with setPrototypeOf, whose index property exists
// already (data, then getter), must be seen by every copy.
// The array-like gets a plain object, the holey array an object that still
// inherits Array.prototype (so slice and spread keep working).
function swapped(label, makeProto) {
  const arrayLike = Object.setPrototypeOf({ length: 4 }, makeProto());
  const holey = Object.setPrototypeOf([1, , 3, , 5], Object.setPrototypeOf(makeProto(), Array.prototype));
  const show2 = (name, f) => {
    let r;
    try {
      r = f();
    } catch (e) {
      r = 'threw ' + (e && e.constructor && e.constructor.name);
    }
    console.log(label + ' ' + name + ' ' + r);
  };
  show2('from(arrayLike)', () => show(Array.from(arrayLike)));
  show2('slice.call(arrayLike)', () => show(Array.prototype.slice.call(arrayLike)));
  show2('concat(arrayLike)', () => show([].concat(arrayLike)));
  show2('from(holey)', () => show(Array.from(holey)));
  show2('slice(holey)', () => show(holey.slice()));
  show2('spread(holey)', () => show([...holey]));
  show2('concat(holey)', () => show([0].concat(holey)));
}
swapped('swap data', () => ({ 1: 'p1' }));
swapped('swap getter', () => ({
  get 1() {
    return 'g1';
  },
}));
// The swap happens after copies of the same shape already ran.
const early = { length: 3 };
console.log('swap before ' + show(Array.from(early)) + ' ' + show(Array.prototype.slice.call(early)));
Object.setPrototypeOf(early, { 2: 'p2' });
console.log('swap after ' + show(Array.from(early)) + ' ' + show(Array.prototype.slice.call(early)));

// An Array subclass gets an indexed property on its prototype after instances exist.
class A extends Array {}
const made = new A();
made.length = 4;
made[0] = 'a0';
const copyOf = (name, f) => console.log('subclass ' + name + ' ' + f());
copyOf('before slice', () => show(made.slice()));
copyOf('before from', () => show(Array.from(made)));
copyOf('before spread', () => show([...made]));
copyOf('before concat', () => show([9].concat(made)));
A.prototype[2] = 'a2';
copyOf('after slice', () => show(made.slice()));
copyOf('after from', () => show(Array.from(made)));
copyOf('after spread', () => show([...made]));
copyOf('after concat', () => show([9].concat(made)));
copyOf('after plain slice.call', () => show(Array.prototype.slice.call(made)));
Object.defineProperty(A.prototype, 3, {
  configurable: true,
  get() {
    return 'getter3';
  },
});
copyOf('getter slice', () => show(made.slice()));
copyOf('getter from', () => show(Array.from(made)));
copyOf('getter concat', () => show([9].concat(made)));
delete A.prototype[2];
delete A.prototype[3];
copyOf('removed slice', () => show(made.slice()));
