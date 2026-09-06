The public js_json_parse wrapper shrinks from 372 instruction addresses and a
0x90-byte frame to 17 instruction addresses and a 0x10-byte frame. Scalar code
now lives in a separate helper; this is an entry-layout change, not a claim
that the complete parser became 22 times smaller or faster. Container calls
avoid six extra pairs of callee-saved register stores and restores. Full worker
hashes and exact objdump commands are recorded in summary.json.
