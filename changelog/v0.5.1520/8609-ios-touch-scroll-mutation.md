### Fixed

- iOS apps no longer abort inside UIKit layout when JavaScript mutates labels or arranged subviews while a `UIScrollView` is being dragged or decelerating. Perry now waits for touch scrolling and its final layout transaction to settle before resuming UI-producing runtime work.
