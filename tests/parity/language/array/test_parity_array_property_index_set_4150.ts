interface State { width: number; mirror: number[] }
function write(state: State, cell: number, value: number): void { state.mirror[cell] = value; }

const state: State = { width: 20, mirror: new Array(40) };
write(state, 0, 112);
write(state, 1, 101);
state.mirror[2] = 114;
if (state.mirror[0] !== 112 || state.mirror[1] !== 101 || state.mirror[2] !== 114) {
  throw new Error(`indexed writes: ${state.mirror[0]}, ${state.mirror[1]}, ${state.mirror[2]}`);
}
console.log("array property index set ok");
