24 fresh-process trials compare the current worker with shape-plans 936c60675bcf8c4cc65d76f48c96937338a4e620, Node and Bun, retaining 1 or 200,000 empty parsed objects. The candidate and reference are measured in the same admitted window. RSS includes the entire process.

| Retained | Engine | RSS after median [range] MiB | Peak RSS median [range] MiB |
|---:|---|---:|---:|
| 1 | baseline | 12.109375 [12.109375, 12.109375] | 13.640625 [13.640625, 13.640625] |
| 1 | perry | 12.328125 [12.328125, 12.328125] | 13.750000 [13.750000, 13.750000] |
| 1 | node | 52.593750 [52.500000, 52.609375] | 53.484375 [53.406250, 53.531250] |
| 1 | bun | 28.296875 [28.296875, 28.312500] | 28.718750 [28.718750, 28.734375] |
| 200000 | baseline | 24.750000 [24.734375, 24.750000] | 26.171875 [26.156250, 26.171875] |
| 200000 | perry | 24.968750 [24.953125, 24.968750] | 26.281250 [26.265625, 26.296875] |
| 200000 | node | 83.421875 [83.406250, 83.484375] | 84.296875 [84.203125, 84.328125] |
| 200000 | bun | 50.203125 [50.187500, 50.281250] | 50.625000 [50.609375, 50.703125] |

The candidate RSS and peak regressions versus this retained reference have separated observed ranges and remain open.
