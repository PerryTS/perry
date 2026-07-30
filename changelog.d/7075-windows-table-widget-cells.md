Added view-backed Table cells on Windows. Render closures can now return Text,
Image, Button, stacks, and other widgets whose native windows are clipped and
positioned inside their ListView cells, including interactive controls that
continue to receive Perry callbacks while the table scrolls and resizes.
