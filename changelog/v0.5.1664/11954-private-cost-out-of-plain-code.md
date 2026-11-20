A shape mint no longer pays for private brands it does not carry (#11791).

Making private brands part of shape identity (#11864) widened the runtime
shape mint enough that its hot helpers stopped being inlined, and every
receiver mint built and dropped an empty brand list. The brandless path is
the cheap path again: the hash, compare, record builders and bucket append
are inline, the brand and ConstFn extension work is out of line, and a
receiver with no brand carries no allocation. Against main with #11864
reverted, Zod goes from +0.23% to -0.21% instructions, qs from +0.34% to
-0.11%, commander to -0.21% and tsc to -0.02%; private field and private
method rows are unchanged.
