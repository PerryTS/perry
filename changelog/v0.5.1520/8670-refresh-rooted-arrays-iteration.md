Refresh rooted arrays during iteration so a collection that relocates the
backing store cannot leave the iterator reading a stale address.
