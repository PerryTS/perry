DEC 250: Cannot add member Object.prototype.missing: built-in objects are read-only in a compiled program
overwrite 0=L0.v 1500=L0.v2
shadow-mid 0=L0.v2 1500=L4.v
own-shadow 0=L4.v 1500=own
delete-own 0=own 1500=L4.v
delete-mid 0=L4.v 1500=L0.v2
delete-holder 0=L0.v2 1500=undefined
re-add-holder 0=undefined 1500=back
method 0=fn:L0.m 1500=fn:L6.m
method-del 0=fn:L6.m 1500=fn:L0.m
method-replace 0=fn:L0.m 1500=fn:L0.m2
getter 0=L0.g 1500=L2.g
data-to-getter 0=back 1500=L5.get
absent-then-mid 0=undefined 1500=L3.missing
absent-holder-undef 0=L3.missing 1500=undefined
absent-holder-null 0=undefined 1500=null
relink-mid 0=L0.v 1500=Alt.v
relink-back 0=Alt.v 1500=L0.v
proxy-in-chain 0=L0.v 1500=proxy.v
null-root 0=undefined
null-root-v 0=L0.v 1500=L1.v
mixin 0=Root.k 1500=Root.k2
mixin-sibling 0=Root.k2 1500=own.k
mixin-evals Root.k2,Root2.k,Root.k2,Root2.k,Root2.k
many L0.v,L0.v,L0.v,L0.v,L0.v,undefined,L1.late,L1.late,L1.late,L1.late,L1.late,undefined
latched-site undefined,undefined,undefined,undefined,undefined,L3.q,L3.q,L3.q
