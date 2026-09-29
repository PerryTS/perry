Megamorphic property reads confirm the site's last slot against the receiver's
shape with one pointer compare: property-key literals are now one string per
key text (key atoms), and shape key lists hold that string. A 40-shape
`o.kind` read drops from ~300 to ~140 instructions. Atoms do not change which
keys count as interned.
