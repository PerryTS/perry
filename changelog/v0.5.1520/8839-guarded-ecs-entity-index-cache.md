Nested stable-packed ECS loops now reuse an admitted receiver proof on call-free paths and cache
repeated reads of the same entity index. Semantic calls invalidate both facts before execution; the
next indexed access reloads the rooted receiver and revalidates it, with an exact generic read on
failure rather than replaying prior iteration effects. This preserves getters, proxies, exceptions,
mutation, and moving-GC behavior while making the unchanged Wolf `simple_iter` kernel 8.82% faster
in an 11-pair controlled cohort (11/11 wins and 30/30 semantic-oracle passes).
