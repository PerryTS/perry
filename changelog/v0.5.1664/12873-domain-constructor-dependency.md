### Fix domain constructor dependency arming

Domain creation now arms its existing domain/events constructor dependency before allocation, including direct native creation paths that bypass namespace installation. This preserves the existing canonical constructor registration and validator.

Scoped original node-suite verification removes all 29 previous constructor aborts: 12 cases now pass and 17 expose semantic parity failures. The domain inventory is 23 PASS and 25 parity failures, not full parity; `create-state` still differs on `undefined` versus `null`. All 11 prior PASS and 8 prior parity failures retain their observed status and raw output.

This is a publication-only transfer of the verified four-line candidate onto merged #12869 with identical pre-change domain source bytes. No new build or test was run on the publication base, and no latest-main or whole-repository equivalence is claimed.
