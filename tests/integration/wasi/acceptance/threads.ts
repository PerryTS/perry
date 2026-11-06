import { parallelMap, parallelFilter, spawn } from "perry/thread";
console.log(parallelMap([1, 2, 3], x => x * 2).join(","));
console.log(parallelFilter([1, 2, 3], x => x > 1).join(","));
spawn(() => 42).then(value => console.log("spawn", value));
