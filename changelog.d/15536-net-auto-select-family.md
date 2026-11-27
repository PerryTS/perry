- **Happy-eyeballs connects.** A connect to a hostname with several addresses
  now races them as Node's `autoSelectFamily` does: an attempt still pending
  after `autoSelectFamilyAttemptTimeout` gives way to the next address, every
  attempt is reported through `connectionAttempt`, `connectionAttemptFailed`
  and `connectionAttemptTimeout`, `autoSelectFamilyAttemptedAddresses` lists
  them, and a socket whose every attempt failed is destroyed with an
  `AggregateError`. `autoSelectFamilyAttemptTimeout` is validated as in Node,
  and with `autoSelectFamily: false` only the address the lookup reports is
  connected.
