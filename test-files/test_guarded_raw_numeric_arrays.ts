// JS-visible parity coverage for guarded raw-f64 array storage.
// The runtime may keep dense numeric arrays unboxed internally, but every
// observable boundary must behave like ordinary JavaScript arrays.

function line(label: string, value: unknown) {
  console.log(label + ":", value);
}

const dense: number[] = [];
for (let i = 0; i < 24; i++) {
  dense.push(i + 0.5);
}
dense[3] = 7;
dense.push(-0);
dense.push(NaN);
line("dense-length", dense.length);
line("dense-sum", dense[0] + dense[3] + dense[23]);
line("dense-negative-zero", Object.is(dense[24], -0));
line("dense-nan", Number.isNaN(dense[25]));

const methodSource = [1, 2, 3, 4, 5];
line("slice", methodSource.slice(1, 4).join(","));
line("concat", methodSource.concat([6, 7]).join(","));
line("reverse", methodSource.slice().reverse().join(","));
line("map", methodSource.map((n) => n * 2).join(","));
line("filter", methodSource.filter((n) => n % 2 === 1).join(","));
const spliced = methodSource.slice();
const removed = spliced.splice(1, 2, 9, 10);
line("splice-removed", removed.join(","));
line("splice-result", spliced.join(","));

const mixed: any[] = [1, 2, 3];
mixed[1] = "two";
mixed.push({ value: 4 });
mixed.push(undefined);
line("mixed-length", mixed.length);
line(
  "mixed-types",
  typeof mixed[0] + "," + typeof mixed[1] + "," + typeof mixed[3] + "," + typeof mixed[4],
);

const sparse: any[] = [1, 2];
sparse[5] = 6;
line("sparse-length", sparse.length);
line("sparse-hole", 3 in sparse);
line("sparse-value", sparse[5]);

const rawSparse: number[] = [];
rawSparse.push(1);
rawSparse.push(2);
rawSparse[5] = 6;
line("raw-sparse-length", rawSparse.length);
line("raw-sparse-hole3", 3 in rawSparse);
line("raw-sparse-v3", rawSparse[3]);
line("raw-sparse-join", rawSparse.join("|"));

const deleted: any[] = [10, 20, 30];
const deleteResult = delete deleted[1];
line("delete-result", deleteResult);
line("delete-length", deleted.length);

console.log("guarded-raw-numeric-arrays: ok");
