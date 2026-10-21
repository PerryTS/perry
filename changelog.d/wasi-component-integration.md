Combine the separately qualified native commands, host resources, deferred
cleanup and current WASI providers under component-host ABI 5. Preserve both
network and host-resource acceptance phases. Add a command/library integration
probe that executes commands from a live library callback under moving GC,
then checks library survival after command traps and fresh commands after
library disposal. Combined execution qualification is pending.
