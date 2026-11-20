Fixed the remaining Windows styling setters that silently discarded their
visual effect. TextField foreground colors now render through the EDIT control
color path, while Button preserves its title and text-backed icon so image
position changes render with the correct ordering and orientation.
