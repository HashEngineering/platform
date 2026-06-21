# Keep JNA classes
-keep class com.sun.jna.** { *; }
-keepclassmembers class * extends com.sun.jna.Structure { *; }
-keepclassmembers interface * extends com.sun.jna.Library { *; }

# Keep SDK public API
-keep class org.dash.sdk.** { *; }
-keepclassmembers class org.dash.sdk.** { *; }
