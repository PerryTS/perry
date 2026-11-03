Prevent `net.Server.listen(port, callback)` from reading callback/object storage
as a native hostname. Address conversion now accepts only actual JS strings and
copies their logical byte length, restoring Linux listen parity without
changing IPv4, IPv6, or unspecified-host behavior.
