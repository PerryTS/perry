Fix inherited static calls through a function superclass using the original class receiver. Object-literal methods now use the existing Function.call/apply rebinding mechanism before invocation, so nested arrows capture the class rather than the superclass's prototype. This removes the receiver-argument-only assumption without adding a dispatch path or cache.

Regression: test-files/test_gap_ocrun2_service_receiver.ts matches Node and Bun; the base prints the prototype key for the class call. OpenCode's Context.Service.use is the witness.
