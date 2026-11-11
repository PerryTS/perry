// process.finalization (Node 22+) is an object exposing register /
// unregister / registerBeforeExit hooks.
console.log("is object:", typeof process.finalization === "object" && process.finalization !== null);
