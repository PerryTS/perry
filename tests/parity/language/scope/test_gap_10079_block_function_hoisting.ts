// #10079: also run by the integration test in explicitly pinned script/ESM packages.
function hoistedVar() {
  const callbacks: Array<() => number> = [];
  for (let index = 0; index < 3; index++) {
    callbacks.push(read);
    var value = index;
    function read() { return value; }
  }
  return callbacks.map(callback => callback()).join(",");
}
console.log("var", hoistedVar());

function lexicalCaptures() {
  const callbacks: Array<() => number> = [];
  for (let index = 0; index < 3; index++) {
    callbacks.push(read);
    let value = index + 10;
    function read() { return value; }
  }
  return callbacks.map(callback => callback()).join(",");
}
console.log("let", lexicalCaptures());

function shadowParameter(read: any) {
  let result = "";
  {
    result = String(read());
    function read() { return 7; }
  }
  return result + ":" + read;
}
console.log("parameter", shadowParameter("outer"));

function annexBUpdates() {
  function outerValue() {
    return typeof read === "function" ? read() : "absent";
  }
  for (let index = 0; index < 2; index++) {
    console.log("before block", outerValue());
    {
      console.log("before declaration", read(), outerValue());
      function read() { return index; }
      console.log("after declaration", outerValue());
      read = () => 90 + index;
      console.log("after local assignment", read(), outerValue());
    }
    console.log("after block", outerValue());
  }
}
annexBUpdates();

function mutual() {
  for (let index = 0; index < 2; index++) {
    console.log("mutual", even(4), odd(4));
    function even(n: number): boolean { return n === 0 || odd(n - 1); }
    function odd(n: number): boolean { return n !== 0 && even(n - 1); }
  }
}
mutual();
