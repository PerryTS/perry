### feat(windows): render MapView with a native MapControl

Windows `MapView` now replaces its text placeholder with a native
`Windows.UI.Xaml.Controls.Maps.MapControl` hosted in a XAML Island. The existing
cross-platform API drives the native center and zoom, titled `MapIcon` pins,
pin clearing, and Road/Aerial/AerialWithRoads styles. The island follows Perry's
Win32 layout and message loop, and is released with its host widget.

Perry's embedded Windows UI manifest now opts unpackaged applications into the
Windows 10 1903 compatibility context required by XAML Islands. Map-service-token
setup through `PERRY_MAP_SERVICE_TOKEN` and `PERRY_BING_MAPS_KEY` is documented.
Fixes #559.
