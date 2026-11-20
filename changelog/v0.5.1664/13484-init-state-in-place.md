Every program starts about 57,000 instructions faster (8.5 % of hello on
x86-64): the per-thread runtime state is now built where it lives. It was
built in stack temporaries and moved into its heap allocation, which cost four
copies of 46 KB in all plus a 16 KB stack fill, and made startup's
instruction count depend on stack alignment. The allocation now comes
zero-filled, so the two inline caches, which are empty as zero bits, are not
written at all.
