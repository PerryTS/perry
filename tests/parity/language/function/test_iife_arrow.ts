const _cjs = (function () {
  const createContext = (value: string) => ({ Provider: "p", value });
  return { createContext };
})();

console.log("typeof _cjs.createContext:", typeof _cjs.createContext);
console.log("got:", _cjs.createContext("x"));
