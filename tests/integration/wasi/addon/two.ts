// Two addons that define the same symbols, entry included: each keeps its own.
import hello from "hello-addon";
import second from "second-addon";

console.log(hello.name(), second.name());
console.log(hello.hello(), "/", second.hello());
