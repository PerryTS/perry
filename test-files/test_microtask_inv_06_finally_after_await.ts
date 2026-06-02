// try/finally around await. The finally body runs after the await resumes,
// in the same continuation. A sibling .then queued after `f()` should land
// AFTER "in try" but the relative order of "in finally" vs further siblings
// matters too.
async function f() {
    try {
        await Promise.resolve();
        console.log("in try");
    } finally {
        console.log("in finally");
    }
    console.log("after");
}
f();
Promise.resolve().then(() => console.log("sib1"));
Promise.resolve().then(() => console.log("sib2"));
console.log("sync");
