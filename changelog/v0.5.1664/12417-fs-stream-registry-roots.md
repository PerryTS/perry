Fix fs stream property publication so JavaScript allocation and GC can run after the stream registry borrow is released, while rooting stream state and callbacks across moving collections.
