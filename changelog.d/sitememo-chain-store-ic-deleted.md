### Removed

- The inherited-access chain-store IC (`object::chain_store`: per-site
  chain-verdict entries, their process-lifetime entry list and its root
  scanner) is deleted. It never served a store on the measured programs
  (hits=0, identical instructions with it off): key-adding stores are served
  by the shape transition fact and the key-add memo. Its allocation-free key
  and chain-marking helpers move to `object::store_key`. The static-key
  store way cache and the write PIC each lose their chain word
  (`PACKED_SET_SETTER_WORD` is now 8; `WRITE_PIC_WORDS` is 8).
