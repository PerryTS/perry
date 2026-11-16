When a bundled codegen unit is kept because a sibling needs a symbol only its
copy defines, the stdlib's replacement unit still loads and both export the
shared symbols, giving a duplicate-symbol link error. The kept member's copy
of the shared exports is now made local, so the stdlib is their single
provider.
