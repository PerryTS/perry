import { Histogram } from "./_helpers/equivalent_shape_histogram.ts";
declare function gc(): void;

// Keep the exported direct-call capabilities live across a module boundary.
// The receiver, array and concatenation temporaries must survive collection
// when score/text use the ordinary implementation through the shape alias.
for (let round = 0; round < 8; round++) {
  const histogram = new Histogram();
  for (let i = 0; i < 1000; i++) histogram.add(i);
  if (typeof gc === "function") gc();
  console.log(histogram.score());
  console.log(histogram.text());
}
