Fixed the whole-arena GC trigger re-arming at a point that depended on which
collector ran (#11736). The non-moving reclaim releases an Eden block after two
collections in a row find it idle; the moving (copying) minor reset every
block's idle count and kept them all, so the same heap re-armed a block higher
for every idle block it kept: 62 MB on TypeScript, worth about 28 MB of peak
RSS whenever the first collection after a full happened to be a moving one.
The moving minor now ages and releases idle Eden blocks by the same rule. Blocks
it filled since the previous collection are its working nursery and are never
released, so a steady copying workload re-maps nothing.
