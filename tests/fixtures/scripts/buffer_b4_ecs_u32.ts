const n = 65536;
const x = new Uint32Array(n);
const y = new Uint32Array(n);
const velocity = new Uint32Array(n);
for (let i = 0; i < n; i++) { x[i] = i; y[i] = 2 * i; velocity[i] = (i % 17) + 1; }
function step(x: any, y: any, velocity: any, frames: number) {
  for (let frame = 0; frame < frames; frame++) {
    for (let i = 0; i < x.length; i++) {
      x[i] = x[i] + velocity[i];
      y[i] = y[i] + velocity[i] * 2;
    }
  }
}
step(x, y, velocity, 100);
let checksum = 0;
for (let i = 0; i < n; i++) checksum += x[i] + y[i];
console.log(checksum);
