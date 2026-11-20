### Fixed

- Convert `ic_miss.rs`'s twelve bare raw-handle reads to `with_{mut,const}_ptr`.
  #8560 added them inside `c3c_pic_tests`; they passed the ratchet at the time
  because the baseline was still 974, and became a violation once #8559's regex
  capture cleanup lowered it to 925.
