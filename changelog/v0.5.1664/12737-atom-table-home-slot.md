The atom table starts each probe from the mixed hash (high half folded in) and grows at half load, so dynamic ASCII keys no longer cluster and computed-key lookups that miss stay short.
