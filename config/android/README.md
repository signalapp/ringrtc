# Android Dependencies for RingRTC Build Time

This configuration defines the build time Java dependencies for RingRTC.

To add/update dependencies edit `build.gradle` and make modifications
to the `dependencies` section.

These dependencies are fetched as part of `prepare_workspace`.

## SDK versions

There are a few places where minSdkVersion and targetSdkVersion show
up in the build of RingRTC for Android.  Generally try to keep them in
sync with Signal-Android/build.gradle.

1. bin/install-ndk-toolchains -- which toolchain to install?
1. src/android/AndroidManifest.xml -- what supported versions in our .aar

