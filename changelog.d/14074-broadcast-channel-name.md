### Fixed

- Preserve omitted versus explicit `undefined` names in `BroadcastChannel` construction, including imported aliases and spread arguments. Reject Symbol names using implicit string conversion before registering a channel.
