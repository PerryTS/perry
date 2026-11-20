Fixed generic `.onClick` handlers on Windows. Text, image, stack, and other
non-button widgets now retain their callbacks and dispatch them from the
widget's native window, including STATIC controls that need click
notifications enabled explicitly.
