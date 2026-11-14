`JSON.stringify` decodes every value by its tag alone. A word with clear top bits that
named a live allocation was serialised as that object (the `is_raw_pointer` arm of the
stringify decoders), so the one Number per live object whose bits equal its address
printed as the object. Such a word is a Number; the arm, its bit pre-filter and the
allocation lookup behind it are deleted.
