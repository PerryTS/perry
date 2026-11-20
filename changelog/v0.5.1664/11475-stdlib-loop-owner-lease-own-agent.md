### Fixed

- perry-stdlib tests: fixed two races between concurrent lib tests (#11472).
  - The loop-owner test lease (`turnloop_client::become_the_owner_for_test`) now runs its holder as its own agent and retires that agent when the lease drops. Before, it used the primary agent. Any concurrent test that turned a loop could claim that agent's route slot, and a thread that loses that race stays `Declined` for its whole life, so the lease's retry loop could never recover. This caused the "never became the primary agent's PUBLISHED loop owner" panic.
  - The TLS turnloop tests now also hold the async_bridge pending-queue test lock. A concurrent `js_stdlib_process_pending` also drains the process-global TLS event queue, and it consumed events such as `listening` before the test could see them.
  - `perry_runtime::agent::enter_agent_for_test` is now public and doc-hidden, so a stdlib test's second thread can join the owner's agent.
