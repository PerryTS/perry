import * as buffer from "node:buffer";
import { atob, btoa } from "node:buffer";

console.log("namespace atob typeof:", typeof buffer.atob);
console.log("namespace btoa typeof:", typeof buffer.btoa);
console.log("namespace atob:", buffer.atob("aGVsbG8="));
console.log("namespace btoa:", buffer.btoa("hello"));
console.log("named atob:", atob("cGVycnk="));
console.log("named btoa:", btoa("perry"));
