# node:os granular parity suite

Focused Node.js parity cases for Perry's `node:os` compatibility layer.

These tests avoid comparing inherently host-dependent or time-varying values directly. Instead, they compare stable Node semantics such as return types, invariants, import forms, constants, and object/array shapes.
