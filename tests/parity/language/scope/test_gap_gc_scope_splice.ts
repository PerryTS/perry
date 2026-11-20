// Scope-backed array bindings must update the scope slot after splice changes
// the array head. Exercise both the specialized and generic method paths.
function typed() {
  let items = [1, 2, 3];
  let count = 4;
  function read() { return `${items.join(',')}:${count}`; }
  function change() {
    items.splice(1, 1, 8, 9);
    count++;
    return read();
  }
  return { read, change };
}
function generic() {
  let items: any = [1, 2, 3];
  let count = 4;
  function read() { return `${items.join(',')}:${count}`; }
  function change() {
    items.splice(1, 1, 8, 9);
    count++;
    return read();
  }
  return { read, change };
}
for (const create of [typed, generic]) {
  const state = create();
  console.log(state.read(), state.change(), state.read());
}
