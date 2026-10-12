class Arena {
  left: Int32Array;
  right: Int32Array;
  size = 0;
  constructor(n: number) { this.left = new Int32Array(n); this.right = new Int32Array(n); }
}
const arena = new Arena(1024);
for (let i = 0; i < 1024; i++) { arena.left[i] = i; arena.right[i] = i * 2; }
function kernel(a: Arena, n: number): number {
  let sum = 0;
  for (let k = 0; k < n; k++) {
    const i = a.size++;
    sum += a.left[i & 1023] + a.right[i & 1023];
  }
  return sum + a.size;
}
console.log(kernel(arena, Number(process.argv[2] || 1000000)));
