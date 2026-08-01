# `node:inspector/promises` entry evidence

All rows use Node 26.5.0 as oracle. `pass`, `diff`, `error`, and `timeout`
describe direct stdout/exit comparison under Deno 2.9.2 and Bun 1.3.14. Perry
classifications reflect the final maintainer release-runner audit on Node 26.5.1
(31/31 exact matches). Node was repeated five times with identical aggregate
evidence for the original fixture set. The added multiple-pending fixture was
validated against Node and Perry only; alternate runtimes were not rerun for
that row.

| Entry                                                                         | Promise contract                       | Node source basis                   |  Deno |     Bun | Perry |
| ----------------------------------------------------------------------------- | -------------------------------------- | ----------------------------------- | ----: | ------: | ----: |
| `events/notification-settlement.ts`                                           | notification before fulfillment        | `inspector.js` dispatch + promisify |  pass |   error |  pass |
| `lifecycle/disconnected.ts`                                                   | pre/post-connect async rejection       | Promise `post` wrapper              |  pass |    diff |  pass |
| `lifecycle/pending-disconnect.ts`                                             | pending-post rejection/order           | `disconnect()` + promisify          | error |    diff |  pass |
| `lifecycle/reconnect.ts`                                                      | post after reconnect                   | API lifecycle                       |  pass |   error |  pass |
| `lifecycle/repeated-sessions.ts`                                              | independent Promise sessions           | multisession contract               |  pass |   error |  pass |
| `lifecycle/sync-control.ts`                                                   | synchronous lifecycle controls         | inherited `Session` methods         |  pass |    diff |  pass |
| `post/argument-validation.ts`                                                 | validation rejects, never throws       | `promisify(post)`                   |  pass | timeout |  pass |
| `post/circular-params.ts`                                                     | serialization Promise rejection        | `post()` JSON dispatch              |  pass |    diff |  pass |
| `post/concurrent-order.ts`                                                    | independent/input-order settlement     | upstream promises test              |  pass |   error |  pass |
| `post/concurrent-rejection.ts`                                                | one rejection does not cancel peers    | Promise post mapping                |  pass |   error |  pass |
| `post/invalid-protocol-params.ts`                                             | protocol -32602 rejection              | inspector response mapping          |  pass |    diff |  pass |
| `post/multiple-pending-disconnect.ts`                                         | all pending posts reject on disconnect | `disconnect()` + promisify          |   n/a |     n/a |  pass |
| `post/optional-params.ts`                                                     | omitted/null/undefined params          | Promise post signature              |  pass |   error |  pass |
| `post/rejection-identity.ts`                                                  | stable reason identity                 | Promise post rejection              |  pass |    diff |  pass |
| `post/settlement-async.ts`                                                    | no same-turn settlement                | promisified callback                |  pass |   error |  pass |
| `post/unknown-command.ts`                                                     | protocol -32601 rejection              | inspector response mapping          |  pass |    diff |  pass |
| `protocol/enable-disable.ts`                                                  | safe empty result shapes               | inspector API examples              |  pass |   error |  pass |
| `protocol/schema-domains.ts`                                                  | Schema Promise payload                 | V8 Schema protocol                  |  pass |   error |  pass |
| `runtime/await-promise.ts`                                                    | fulfilled awaited evaluation           | inspector API examples              |  pass |   error |  pass |
| `runtime/await-rejection.ts`                                                  | rejection resolves exception details   | V8 Runtime protocol                 |  pass |   error |  pass |
| `runtime/exception-details.ts`                                                | throw resolves exception details       | V8 Runtime protocol                 |  pass |   error |  pass |
| `runtime/get-properties-release.ts`                                           | object lifecycle/rejection             | V8 Runtime protocol                 |  pass |   error |  pass |
| `runtime/numeric-specials.ts`                                                 | special/BigInt result shapes           | V8 Runtime protocol                 |  pass |   error |  pass |
| `runtime/primitives.ts`                                                       | primitive result shapes                | inspector docs                      |  pass |   error |  pass |
| `runtime/return-by-value.ts`                                                  | object/array by-value results          | V8 Runtime protocol                 |  pass |   error |  pass |
| `surface/constructor.ts`                                                      | constructor call/extra argument        | subclass definition                 |  pass |   error |  pass |
| `surface/exports.ts`                                                          | keys/descriptors/import identity       | module object spread                |  diff |    diff |  pass |
| `surface/inherited-receivers.ts`                                              | synchronous receiver brands            | inherited `Session` methods         |  pass |    diff |  pass |
| `surface/post-descriptor.ts`                                                  | promisified method descriptor          | `promisify(post)` assignment        |  pass |    diff |  pass |
| `surface/post-receiver.ts`                                                    | receiver error becomes rejection       | `promisify(post)`                   |  pass |    diff |  pass |
| `surface/session-class.ts`                                                    | subclass/prototype/method identity     | Promise Session definition          |  pass |    pass |  pass |
| No Node fixture failed or timed out. Perry produced no compile failures or    |                                        |                                     |       |         |       |
| execution timeouts. After the alternate-runtime timeout and all focused runs, |                                        |                                     |       |         |       |
| process, endpoint, and generated-artifact scans found no live suite process,  |                                        |                                     |       |         |       |
| listening inspector endpoint, or repository artifact.                         |                                        |                                     |       |         |       |
