// A program that observes none of the lazily born globals still runs.
const xs = [3, 1, 2].sort((a, b) => a - b);
console.log(Math.floor(2.5), JSON.stringify({ xs }), Number("7") + 1, String(xs.length));
