Bundle the Android Gradle template, Kotlin/Java bridge, and resources in the
compiler so `perry run android --local` and Wear OS packaging work from release
installs without a Perry source checkout. Build-time collection tracks new
resources while excluding Gradle caches, local SDK paths, generated output, and
JNI binaries. Includes extraction, resource-fidelity, collector, and Wear OS
regression coverage; the extracted template was built into a debug APK with
Gradle 9.4, JDK 21, and Android SDK 35.
