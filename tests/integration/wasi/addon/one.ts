// One addon linked into a WASI component: a function and a typed array.
import hello from "hello-addon";

console.log(hello.hello());
const bytes = hello.bytes();
console.log(bytes instanceof Uint8Array, bytes.length, bytes[0], bytes[2]);
