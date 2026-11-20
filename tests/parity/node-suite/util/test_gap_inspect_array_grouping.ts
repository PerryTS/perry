// util.inspect lays arrays out with node's groupArrayElements: more than six short
// entries form columns, long entries print one per line, and an array that fits
// stays on one line. Nested arrays follow the same rules at their indentation.
import util from "node:util";

const show = (v: unknown) => console.log(util.inspect(v));

const names = ["OrchestrationEvents", "OrchestrationCommandReceipts", "CheckpointDiffBlobs",
  "ProviderSessionRuntime", "Projections", "ProjectionThreadSessionRuntimeModeColumns",
  "ProjectionThreadMessageAttachments", "ProjectionThreadActivitySequence"];
show({ migrations: names.map((n, i) => `${i + 1}_${n}`) });
show(["aaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbb", "c", "d", "e", "f", "g"]);
show(Array.from({ length: 30 }, (_, i) => i * 1000));
show([1, 2, 3, 4, 5, 6]);
show([1, 2, 3, 4, 5, 6, 7]);
show(Array.from({ length: 26 }, (_, i) => String.fromCharCode(97 + i)));
show([1, "a", true, null, 2, "bb", 3, "c"]);
show([1n, 22n, 333n, 4444n, 5n, 66n, 777n]);
show(["alpha", "beta", "gamma", "delta", "epsilon", "zeta"]);
show(["alpha", "beta", "gamma", "delta", "epsilon", "zeta", "eta", "theta", "iota", "kappa", "lambda"]);
show({ nested: Array.from({ length: 20 }, (_, i) => i), words: ["one", "two", "three", "four", "five", "six", "seven"] });
show([[1, 2, 3], [4, 5, 6], [7, 8, 9], [10, 11, 12], [13, 14, 15], [16, 17, 18], [19, 20, 21]]);
show(Array.from({ length: 12 }, (_, i) => "x".repeat(i)));
show(["it's", 'say "hi"', "tab\there", "line\nbreak"]);
show([`${"x".repeat(50)}\n${"y".repeat(40)}`, "z"]);
show({ text: `${"p".repeat(70)}\nq` });
