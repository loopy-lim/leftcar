# Add project specific ProGuard rules here.
# By default, the flags in this file are appended to flags specified
# in /usr/local/Cellar/android-sdk/24.3.3/tools/proguard/proguard-android.txt
# You can edit the include path and order by changing the proguardFiles
# directive in build.gradle.
#
# For more details, see
#   http://developer.android.com/guide/developing/tools/proguard.html

# react-native-reanimated
-keep class com.swmansion.reanimated.** { *; }
-keep class com.facebook.react.turbomodule.** { *; }

# Add any project specific keep options here:

# Expo / React Native release minify — 리플렉션으로 로드되는 클래스 보존.
-keep class expo.modules.** { *; }
-keep class * implements expo.modules.core.InternalModule { *; }
-keep class * implements expo.modules.core.ExportedModule { *; }
-dontwarn expo.modules.**
-keep class com.facebook.react.** { *; }
-dontwarn com.facebook.react.**
-keepclassmembers class * { native <methods>; }
-keep class dev.leftcar.viewer.km.KmInputAuthority { *; }
-keep class dev.leftcar.viewer.km.IKmInputAuthority$Stub { *; }

# Exact targeted own-service member adapter; bundled consumer rules preserve ART helpers.
-keep class org.lsposed.hiddenapibypass.HiddenApiBypass {
    public static java.lang.Object invoke(java.lang.Class, java.lang.Object, java.lang.String, java.lang.Object[]);
    public static java.util.List getInstanceFields(java.lang.Class);
}
