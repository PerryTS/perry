24 fresh-process trials compare the current worker with escaped-count 4b9d577d4e81518f83a8b4a1e4f319d328597f74, Node and Bun, retaining 1 or 200,000 empty parsed objects. The candidate and reference are measured in the same admitted window. RSS includes the entire process.

| Retained | Engine | RSS after median [range] MiB | Peak RSS median [range] MiB |
|---:|---|---:|---:|
| 1 | baseline | 12.031250 [12.031250, 12.046875] | 13.562500 [13.562500, 13.578125] |
| 1 | perry | 12.328125 [12.328125, 12.343750] | 13.750000 [13.750000, 13.765625] |
| 1 | node | 52.609375 [52.531250, 52.625000] | 53.500000 [53.453125, 53.515625] |
| 1 | bun | 28.265625 [28.250000, 28.281250] | 28.687500 [28.671875, 28.687500] |
| 200000 | baseline | 24.625000 [24.625000, 24.640625] | 26.062500 [26.062500, 26.078125] |
| 200000 | perry | 24.968750 [24.968750, 24.968750] | 26.296875 [26.281250, 26.296875] |
| 200000 | node | 83.406250 [83.296875, 83.437500] | 84.203125 [84.109375, 84.265625] |
| 200000 | bun | 50.203125 [50.203125, 50.234375] | 50.625000 [50.625000, 50.656250] |

The candidate RSS and peak regressions versus this retained reference have separated observed ranges and remain open.
