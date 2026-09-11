# R43: bound dominant-token parser selection

R42 outlining reduced wide parse's regression from R41 +4.29% to +3.19% against R26, but seven targeted rows still regress. Select the compact false specialization for large documents where a borrowed dominant token cannot be useful. Keep <=256 false and 257..4096 true. For larger inputs, a bounded 512-byte quote/backslash scan can prove that no unescaped token opened before byte 256 remains open. Both real parsers retain complete validation.

The standalone eligibility sketch passed 137,630 checks. Integrate it with an outlined large-input probe so the loop does not enter the small-document frame. Add complete parse/stringify and malformed-input controls across both branches, and extend existing UTF-16/flag boundary tests. No GC policy, roots, parser boundary scheduling, cache admission or memory cap changes.

Run serial JSON/string tests, full lint, a normal three-package build and canonical full-output/GC/IR validation. Measure all twelve R42 targeted controls, then fresh ASCII/Unicode/escaped/small inputs and Korean strings if the candidate merits broadening. A final integrated candidate still needs the full suite and merged-main measurement. No production speed claim from the standalone eligibility sketch.
