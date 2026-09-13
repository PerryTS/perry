Fix computed property reads and `Object.entries` / `Object.values` for parsed
objects with more than 10,000 inline fields. Indexed reads now use the
object's published live-slot bound without rejecting otherwise valid wide
objects. Out-of-range indices still follow the existing overflow lookup.
