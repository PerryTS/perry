Fixed an absent property read costing `2^depth` generic-getter entries on a
prototype chain (#10877). The object-getter tail read the receiver's prototype
twice on an own-key miss, and each read re-enters the tail one level up, so the
work doubled per hop:

- **`Object.create` chains**: `prototype_override::inherited_field_if_overridden`
  walked the recorded chain, missed, and the tail's closing
  `resolve_inherited_field` walked the same chain again from the same
  receiver. It now reports `InheritedRead::Missed` and the tail skips its own
  walk.
- **`F.prototype = new G()` chains**: a `new F()` instance reaches
  `F.prototype` both through F's synthetic class id
  (`resolve_proto_chain_field_*`) and through its recorded per-object link
  (`resolve_inherited_field`). The class-id walk now reports a prototype it read
  in full that answered exactly `undefined`
  (`resolve_proto_chain_field_noting_miss`); the tail skips the per-object walk
  when that is the receiver's recorded prototype. A `null` answer does not
  qualify — the class-id walk skips `null`, which the per-object read must
  still return.

Observable effect beyond speed: an inherited getter that returns `undefined`
ran `2^depth` times per read (once per re-entry); it now runs once, as in node.
`class C extends B` chains were already linear and are unchanged.

Measured (`perry-dev`, 20,000 absent reads, min of 3 wall-clock):

| chain | depth | before | after |
|---|---:|---:|---:|
| `Object.create` | 1 | 0.103 s | 0.046 s |
| `Object.create` | 4 | 0.803 s | 0.075 s |
| `Object.create` | 8 | 12.50 s | 0.120 s |
| `Object.create` | 10 | 51.4 s | 0.151 s |
| `F.prototype = new G()` | 1 | 0.115 s | 0.054 s |
| `F.prototype = new G()` | 4 | 0.833 s | 0.082 s |
| `F.prototype = new G()` | 8 | 13.52 s | 0.132 s |
| `F.prototype = new G()` | 10 | > 60 s | 0.156 s |

Regression coverage: `test-files/test_gap_10877_proto_chain_miss_linear.ts`
pins exact getter call counts on both chain kinds (keyless and shaped
receivers), a `null`-valued inherited property, hits and middle links, and a
1,000-miss loop on 24-deep chains that did not finish in 120 s before.
