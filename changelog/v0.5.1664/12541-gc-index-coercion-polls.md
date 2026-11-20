The GC root-dominance checker now treats the string index conversion helpers as poll-capable because their tagged-value inputs can invoke `ToNumber`.
