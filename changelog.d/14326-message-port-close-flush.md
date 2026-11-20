### Fixed
- Preserve the MessagePort inbox while an active dispatch drains after `close()`, including synchronous receives and newly queued peer messages, while rejecting transfers of the closing port.
