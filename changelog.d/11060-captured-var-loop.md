### Fixed

- Function-local `var` loop counters captured by closures now keep their condition and update on the same shared binding, preventing `for` loops from running forever. (#11052)
