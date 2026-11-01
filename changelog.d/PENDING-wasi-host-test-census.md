Update native WASI host qualification to require all five loop-interrupt tests,
including the input-terminator regression, instead of failing after the five
passing tests because the workflow still expected four.
