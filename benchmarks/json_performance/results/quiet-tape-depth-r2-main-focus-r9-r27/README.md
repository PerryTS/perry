# Tape-depth R2 focused replay

Nine repetitions per case, with 27 for Unicode stringify, and three randomized Perry arms: tape-depth R2, freshly built merged main eee3881c4, and tape-depth R1. Node is the output oracle. All 48 output checks and 378 timing trials passed, under the archived quiet-host window. CPU is process CPU per call, with all samples retained. Differences are descriptive; overlapping ranges do not prove equality.

| Fixture | Operation | Candidate us | Main us | R1 us | vs main | vs R1 | Slower pairs | Peak delta KiB |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| records_array_16k | parse | 13.633100 | 18.111600 | 15.115300 | -24.727% | -9.806% | 0/9 | +32 |
| records_array_16k | sparse | 21.293100 | 25.138250 | 22.210950 | -15.296% | -4.132% | 0/9 | +64 |
| records_array_16k | scan | 72.058900 | 76.969700 | 73.899800 | -6.380% | -2.491% | 0/9 | +112 |
| records_array_1m | parse | 909.046875 | 1189.335938 | 1005.554688 | -23.567% | -9.597% | 0/9 | +32 |
| heterogeneous_1m | parse | 1110.732422 | 1392.888672 | 1207.939453 | -20.257% | -8.047% | 0/9 | +48 |
| small_record | parse | 0.096557 | 0.097014 | 0.096401 | -0.471% | +0.162% | 0/9 | +160 |
| small_record | stringify | 0.042996 | 0.043024 | 0.043068 | -0.066% | -0.166% | 5/9 | +192 |
| long_string_1m | stringify | 31.269012 | 32.820587 | 32.415527 | -4.727% | -3.537% | 4/9 | +160 |
| unicode_1m | stringify | 26.299622 | 26.387726 | 26.375671 | -0.334% | -0.288% | 13/27 | +144 |
| unicode_1m | parse | 0.371425 | 0.370504 | 0.371752 | +0.249% | -0.088% | 8/9 | +144 |
| tiny_object | parse | 0.034048 | 0.034584 | 0.034381 | -1.550% | -0.969% | 0/9 | +160 |
| wide_1m | parse | 2867.007812 | 2868.625000 | 2877.062500 | -0.056% | -0.349% | 3/9 | +144 |
