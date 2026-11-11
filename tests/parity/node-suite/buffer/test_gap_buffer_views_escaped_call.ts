// An escaped call can materialize, mutate or detach the backing after the
// caller has established a tracked view. Later reads must use live storage.
function mutate(view: Float64Array): void {
  const sibling = new Float64Array(view.buffer);
  sibling[0] = 41.5;
}

function detach(view: Float64Array): void {
  view.buffer.transfer();
}

function escapedCall(): void {
  const view = new Float64Array(2);
  view[0] = 7.25;
  const before = view[0];
  // Spread deliberately exercises the escape shape in the proof test.
  mutate(...([view] as [Float64Array]));
  console.log("mutated", before, view[0]);
  detach(view);
  console.log("detached", view[0], view[1], view.length);
}

escapedCall();
