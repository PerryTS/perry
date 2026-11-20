Async functions build their result promises on a leaf allocation arm (no parent,
no hooks), keep temporaries that live and die inside one resume state out of
the heap frame (no slot store, no write barrier), and admit settled
continuations with a shorter check. Microtask order and every observable are
unchanged; the async benchmark drops about 21% instructions.
