import { Histogram } from "./_helpers/equivalent_shape_histogram.ts";
declare function gc(): void;

const histogram = new Histogram();
for (let i = 0; i < 1000; i++) histogram.add(i);
const score = histogram.score;
const text = histogram.text;
for (let i = 0; i < 3; i++) {
  if (typeof gc === "function") gc();
  console.log(histogram.score(), score.call(histogram));
  console.log(histogram.text(), text.call(histogram));
}
console.log(score === histogram.score, score !== text, score.name, score.length);

// The shared implementation must keep its collecting and throwing array
// access path, including relocation of the live receiver/array on unwind.
Object.defineProperty(histogram.bins, "2", {
  configurable: true,
  get() {
    if (typeof gc === "function") gc();
    throw new Error("histogram getter");
  },
});
try { histogram.score(); }
catch (error) { console.log((error as Error).message); }
try { score.call(histogram); }
catch (error) { console.log((error as Error).message); }
