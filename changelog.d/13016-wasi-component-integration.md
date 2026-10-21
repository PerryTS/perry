Combine the separately qualified native commands, host resources, deferred
cleanup and current WASI providers under component-host ABI 5. Preserve both
network and host-resource acceptance phases. Add a command/library integration
probe that executes commands from a live library callback under moving GC,
then checks library survival after command traps and fresh commands after
library disposal. The combined Linux profile passes 82 core unit tests,
12 weak-reference/finalization regressions and all 71 native/command/guest
acceptance phases. Keep the full weakref/finreg selections in the required
component gate. Matching native runtime and engine archives are required;
HTTP, P3 and blocking-I/O cancellation remain outside this qualification.
