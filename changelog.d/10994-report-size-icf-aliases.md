### Fixed

- `perry compile --report-size` counts linker-folded symbol aliases once per address, preventing inflated crate totals and duplicate-body savings suggestions ([#10994](https://github.com/PerryTS/perry/pull/10994)).
