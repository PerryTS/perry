### Fixed

- Normalize `net.Socket.end` overloads before forwarding data to `write`,
  preventing an end callback from being registered for both write and end.
  Preserve genuine UTF-8 encoding arguments and legitimate repeated callback registrations.
