// Mimic CJS wrap pattern: IIFE that returns an object with function fields
const _cjs = (function () {
  function createContext(value: string) {
    return { Provider: "p", Consumer: "c", value };
  }
  return { createContext };
})();

console.log("typeof _cjs:", typeof _cjs);
console.log("typeof _cjs.createContext:", typeof _cjs.createContext);
const got = _cjs.createContext("hello");
console.log("typeof got:", typeof got);
console.log("got:", got);
