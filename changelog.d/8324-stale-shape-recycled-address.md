Fixed garbage-collector shape metadata so a dead keys-array address reused by a different object type cannot be retained or rekeyed onto the replacement object during a copying minor.
