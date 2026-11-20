### Fixes

- **Padding now reaches native Apple leaf widgets instead of silently stopping
  at stacks.** Text fields and secure fields use inset-aware editing and
  placeholder rectangles, text areas and scroll views use their native content
  insets, and UIKit buttons use `contentEdgeInsets`. Text-field intrinsic sizes
  grow with the requested padding so padded content is not clipped.
