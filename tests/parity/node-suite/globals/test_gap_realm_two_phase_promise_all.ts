// Two-phase realm: a promise-heavy program that names no phase-B global, so
// its realm is never completed. Every element's `Promise.resolve` asks the
// runtime which constructor `Promise` is; with the switch on that question
// must cost what it costs with it off (the measured case of the lane).
async function main(): Promise<void> {
  const xs = Array.from({ length: 200000 }, (_, i) => i);
  const out = await Promise.all(xs.map((x) => Promise.resolve(x)));
  console.log("all", out.length, out[0], out[199999], out.reduce((a, b) => a + b, 0));
  const boxed: any = Object("s");
  console.log("boxed", boxed.constructor === String, Object(1).constructor === Number, typeof Promise);
}
main().then(() => console.log("done"));
