// E20: an instance stored on globalThis (unmodeled code can reach it).
// WPFACTS: fact F3 prefix_stable Glob = 0
class Glob {
  g: number = 1;
}
const gl = new Glob();
(globalThis as any).shared = gl;
(globalThis as any).shared.g = "changed";
console.log(gl.g);
