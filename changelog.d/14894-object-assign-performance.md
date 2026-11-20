### Performance

- Accelerate wide `Object.assign` copies by skipping read-index derivation for cold layouts, reading own-property descriptor facts without allocating descriptors, and routing assignments with no inline room left through the guarded keyed store. On the `wide_object_membership/object-assign` benchmark a 10,000-property copy drops from 76.3 ms to 1.6 ms (Node: 1.5 ms) and peak RSS from 1.09 GB to 38 MB; 64- to 1,000-property copies run 2-3x faster, small copies are unchanged.
