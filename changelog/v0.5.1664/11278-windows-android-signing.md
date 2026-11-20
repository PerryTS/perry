Fix Android APK signing on Windows by discovering SDK `.exe`/`.bat` tools,
using the Windows SDK default under LOCALAPPDATA, and locating the debug
keystore in the platform user home. Explicit ANDROID_HOME/ANDROID_SDK_ROOT
overrides remain supported. Alignment failures now stop signing with a useful
error instead of proceeding to an invalid APK. Includes SDK-layout and signing
pipeline tests plus Windows batch-path coverage; real SDK alignment and APK
v2/v3 signature verification passed on macOS.
