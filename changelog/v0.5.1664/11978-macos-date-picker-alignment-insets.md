Fixed a macOS `DatePicker` with a background or border drawing 3pt past the
right edge of the stack it fills and 4pt above its slot (#11966). AppKit gives
a stock `NSDatePicker` alignment rect insets of top 4 and right 3, and Auto
Layout pins the alignment rect, so the frame that carries the layer decoration
overhung the slot. The picker now reports zero insets and hugs a stock picker's
alignment rect. Its cell draws the control and its focus ring in the frame grown
by the stock insets, so the field and stepper stay where a stock picker draws
them. Clicks follow, because the cell hit-tests against the layout from its
last draw.
