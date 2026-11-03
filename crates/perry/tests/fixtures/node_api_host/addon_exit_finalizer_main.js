// FIXTURE_MODE=exit: a referenced thread-safe function and an environment
// cleanup hook are live when process.exit(3) runs; Node exits with 3 and runs
// neither. Otherwise the function is unref'd and the loop ends naturally; the
// environment teardown runs the finalizer and the hook.
const addon = require("fixture-exit-finalizer");
const exit = process.env.FIXTURE_MODE === "exit";
addon.hold(exit);
console.log("held");
if (exit) {
  setTimeout(() => process.exit(3), 0);
}
