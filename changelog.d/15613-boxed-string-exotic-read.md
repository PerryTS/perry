Fix: reading an index of a `String` wrapper (`new String("abc")["1"]`) returns its own character even when its prototype defines that index.
