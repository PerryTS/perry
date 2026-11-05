// A RegExp literal in a program that uses worker threads: string handles are
// thread-local there, and the literal's static site must not name them.
import { isMainThread } from "worker_threads";

const segments = /[^.[\]]+|\[([^[\]]*?)\]/g;
console.log(isMainThread, "a.b[c]".match(segments), "xax".replace(/x/g, "y"));
for (let i = 0; i < 3; i++) console.log(/(\d+)-(\d+)/.exec("10-20")?.[2], segments.test("p.q"));
