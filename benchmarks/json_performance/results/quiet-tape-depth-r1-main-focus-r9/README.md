# Tape-depth R1 focused replay

Nine repetitions per case and three randomized Perry arms: tape-depth R1, freshly built merged main eee3881c4, and opening-scan R3. Node is the output oracle. All 36 output checks and 243 timing trials passed, under the archived quiet-host window. CPU is process CPU per call, with all samples retained. Differences are descriptive; overlapping ranges do not prove equality.

| Fixture | Operation | Candidate us | Main us | R3 us | vs main | vs R3 | Slower pairs / 9 | Peak delta KiB |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| records_array_16k | parse | 15.107550 | 18.121850 | 18.151550 | -16.634% | -16.770% | 0 | +16 |
| records_array_16k | sparse | 22.207650 | 25.147000 | 25.203450 | -11.689% | -11.886% | 0 | +16 |
| records_array_16k | scan | 73.999900 | 77.114900 | 76.971500 | -4.039% | -3.861% | 0 | +176 |
| records_array_1m | parse | 1006.292969 | 1190.322266 | 1191.097656 | -15.460% | -15.515% | 0 | +16 |
| heterogeneous_1m | parse | 1208.304688 | 1392.277344 | 1395.113281 | -13.214% | -13.390% | 0 | +16 |
| small_record | parse | 0.096445 | 0.097088 | 0.095922 | -0.663% | +0.544% | 0 | +96 |
| small_record | stringify | 0.043037 | 0.043023 | 0.043025 | +0.032% | +0.027% | 6 | +96 |
| long_string_1m | stringify | 31.220154 | 32.048309 | 32.483551 | -2.584% | -3.889% | 3 | +144 |
| unicode_1m | stringify | 27.422058 | 25.621582 | 26.601715 | +7.027% | +3.084% | 7 | +112 |
