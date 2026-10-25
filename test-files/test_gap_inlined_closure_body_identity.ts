function factory() { return (input: any) => input.layer; }
function indirect() { return factory(); }
const first = factory();
const second = indirect();
console.log(first({ layer: "first", deps: [] }), second({ deps: [], layer: "second" }));
