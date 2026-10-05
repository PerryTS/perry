Added IR witnesses for the `idxset.bounded` and `private_field_set` write
barriers, and corrected four GC store-site audit markers. No store lacked a
barrier it needs.
