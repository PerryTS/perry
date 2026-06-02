import { format } from "node:util";
console.log("none:", format());
console.log("only:", format("100%% done"));
console.log("extra:", format("%s", "a", "b"));
