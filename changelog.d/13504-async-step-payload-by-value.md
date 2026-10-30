Async functions resume in place with the awaited payload handed back by value
from the settled-continuation probe (a declined probe answers with the TDZ
sentinel), instead of a bool plus a second read of the iter-result cell, and
`Promise.all` skips the per-element resolve and then probes for plain native
promise elements (native promise, no metadata). Microtask order and every
observable are unchanged; the async benchmark drops about 11.7% instructions.
