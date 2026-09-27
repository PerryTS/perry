Avoid emitting guarded array and dynamic typed-array store blocks after an operand has already thrown.
Captured arrays, module globals, and property receivers now preserve the
terminated control-flow path, including type-erased specialized functions, matching the existing local-array store behavior.
This fixes invalid LLVM IR from node-forge's Worker initialization loop and lets
unexecuted unsupported Worker paths coexist with usable native package code.

Regression coverage checks the generated store blocks for throwing and ordinary
operands and exercises captured-array throws, cold paths, and successful writes.
