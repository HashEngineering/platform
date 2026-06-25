# JNA uses reflection/JNI to bind native methods; keep its classes and our FFI structs.
-keep class com.sun.jna.** { *; }
-keep class * extends com.sun.jna.** { *; }
-keep class org.dash.sdk.ffi.** { *; }
-dontwarn java.awt.**
