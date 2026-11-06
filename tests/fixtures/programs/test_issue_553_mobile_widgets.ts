// Issue #553 — exercise the four new perry/ui widgets so the API gate
// flips and the dispatch tables resolve every symbol. The widgets'
// pixel-correctness is platform-specific and validated visually on a
// device; this file just validates that the symbols link and the FFI
// shapes line up.
import {
  BottomNavigation,
  bottomNavAddItem,
  bottomNavSetBadge,
  bottomNavSetSelected,
  ImageGallery,
  imageGalleryAddImage,
  imageGallerySetIndex,
  ScrollView,
  scrollviewSetScrollEndCallback,
  scrollviewSetRefreshControl,
  scrollviewEndRefreshing,
  LazyVStack,
  lazyvstackSetRefreshControl,
  lazyvstackEndRefreshing,
  lazyvstackSetScrollEndCallback,
  Text,
  VStack,
  App,
} from "perry/ui";

// --- BottomNavigation --------------------------------------------------------
const nav = BottomNavigation((index: number) => {
  console.log("nav selected:", index);
});
bottomNavAddItem(nav, "house.fill", "Home");
bottomNavAddItem(nav, "magnifyingglass", "Search");
bottomNavAddItem(nav, "plus.circle.fill", "New");
bottomNavAddItem(nav, "bell.fill", "Notifications");
bottomNavAddItem(nav, "person.fill", "Profile");
bottomNavSetBadge(nav, 3, "12");
bottomNavSetSelected(nav, 0);

// --- ImageGallery ------------------------------------------------------------
const gallery = ImageGallery((index: number) => {
  console.log("gallery index:", index);
});
imageGalleryAddImage(gallery, "/tmp/sample-1.jpg", "Sample 1");
imageGalleryAddImage(gallery, "/tmp/sample-2.jpg", "Sample 2");
imageGalleryAddImage(gallery, "/tmp/sample-3.jpg", "Sample 3");
imageGallerySetIndex(gallery, 0);

// --- ScrollView with pull-to-refresh + scroll-end ---------------------------
const scroll = ScrollView();
scrollviewSetRefreshControl(scroll, () => {
  console.log("pull-to-refresh fired");
  scrollviewEndRefreshing(scroll);
});
scrollviewSetScrollEndCallback(scroll, () => {
  console.log("scroll-end fired (load more)");
}, 200);

// --- LazyVStack with pull-to-refresh + scroll-end ----------------------------
const lazy = LazyVStack(50, (i: number) => Text(`row ${i}`));
lazyvstackSetRefreshControl(lazy, () => {
  console.log("lazy pull-to-refresh fired");
  lazyvstackEndRefreshing(lazy);
});
lazyvstackSetScrollEndCallback(lazy, () => {
  console.log("lazy scroll-end fired");
}, 5);

console.log("issue #553 widgets wired:", typeof nav, typeof gallery, typeof scroll, typeof lazy);

App({
  title: "Issue #553 smoke test",
  width: 400,
  height: 700,
  body: VStack(nav, gallery, scroll, lazy),
});
