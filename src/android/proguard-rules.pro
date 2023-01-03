# Add project specific ProGuard rules here.
# By default, the flags in this file are appended to flags specified
# in /usr/local/android-sdk-linux/tools/proguard/proguard-android.txt
# You can edit the include path and order by changing the proguardFiles
# directive in build.gradle.
#
# For more details, see
#   http://developer.android.com/guide/developing/tools/proguard.html

-dontwarn org.webrtc.NetworkMonitorAutoDetect
-dontwarn android.net.Network
-dontwarn android.support.v4.media.AudioAttributesCompat
-dontwarn android.support.v4.media.AudioAttributesImplApi21
-dontwarn android.support.v4.media.AudioAttributesImplBase
-keep class org.webrtc.** { *; }
-keep class org.signal.ringrtc.** { *; }
