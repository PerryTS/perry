// process.env is a string→string map; reads of a present var are strings and
// reads of an absent var are undefined.
console.log("env object:", typeof process.env === "object");
console.log("PATH is string:", typeof process.env.PATH === "string");
console.log("absent is undefined:", process.env.PERRY_DEFINITELY_UNSET_VAR === undefined);
