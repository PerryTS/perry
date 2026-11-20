Fixed `fetch` inside a Worker never settling (no response, no error, no abort).
A worker now turns its own network loop while it has I/O in flight and settles
its own native completions, and a message sent to a busy worker still wakes it.
