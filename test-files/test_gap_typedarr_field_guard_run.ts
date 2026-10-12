// One class-field receiver guard per straight-line run (class_guard_memo):
// a later access in the run may skip its shape compare only while no call
// has run since the last passing guard. Each case below reshapes the receiver
// from user code reached inside the run, so a stale proof would read or
// write the wrong slot.
class Arena {
  left: Int32Array;
  right: Int32Array;
  size: number = 0;
  constructor(n: number) {
    this.left = new Int32Array(n);
    this.right = new Int32Array(n);
  }
}

function put(a: Arena, l: number, r: number): number {
  const i = a.size++;
  a.left[i] = l;
  a.right[i] = r;
  return i;
}

function get(a: Arena, i: number): number {
  const l = a.left[i];
  const r = a.right[i];
  return l * 100 + r + a.size;
}

const a = new Arena(8);
console.log('plain', put(a, 1, 2), put(a, 3, 4), a.left[1], a.right[1], get(a, 1));

// The element store of a lying `left` deletes and re-adds `right`, which
// moves it to another slot; `a.right` after it must re-prove the shape.
const b = new Arena(8);
const moved = new Int32Array(8);
(b as any).left = new Proxy([] as any, {
  set(target: any, key: any, value: any) {
    delete (b as any).right;
    (b as any).extra = 'x';
    (b as any).right = moved;
    target[key] = value;
    return true;
  },
});
console.log('reshaped', put(b, 5, 6), moved[0], (b as any).extra, b.size);

// The element store turns the raw-f64 `size` lane into a string.
const c = new Arena(4);
(c as any).left = {
  set 0(v: number) {
    (c as any).size = 'str' + v;
  },
};
console.log('lane', put(c, 7, 8), c.right[0], c.size);

// A read whose element getter reshapes the holder between two reads.
const d = new Arena(4);
d.right[0] = 9;
(d as any).left = {
  get 0() {
    delete (d as any).left;
    (d as any).left = new Int32Array(4);
    return 3;
  },
};
console.log('getter', get(d, 0), d.left[0]);

// Lying containers of every other shape keep their own semantics.
const e = new Arena(4);
(e as any).left = [10, 20];
(e as any).right = {};
console.log('array', put(e, 11, 12), (e as any).left[0], (e as any).right[0], get(e, 1));
