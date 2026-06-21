plugins {
    alias(libs.plugins.android.library)
    alias(libs.plugins.kotlin.android)
}

android {
    // Distinct namespace per Android library module (platform vs unified) so their
    // generated R classes do not collide when both are in the same build.
    namespace = "org.dash.sdk.platform"
    compileSdk = 35

    defaultConfig {
        minSdk = 24

        // ABI filters - match what build_platform_android.sh produces.
        // 64-bit only: the unified FFI library carries hard-coded 64-bit struct
        // size/alignment guards (matching the iOS aarch64-only framework), so the
        // 32-bit ABIs (armeabi-v7a / x86) are intentionally unsupported.
        ndk {
            abiFilters += listOf("arm64-v8a", "x86_64")
        }

        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
        consumerProguardFiles("consumer-rules.pro")
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                "proguard-rules.pro"
            )
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    kotlinOptions {
        jvmTarget = "17"
    }

    // Pre-built .so files from build_platform_android.sh are placed in src/main/jniLibs.
    // PLATFORM flavor ships only librs_sdk_ffi.so.
    sourceSets {
        getByName("main") {
            jniLibs.srcDirs("src/main/jniLibs")
        }
    }

    packaging {
        jniLibs {
            // JNA ships its own native libs; keep only the ones we need
            excludes += listOf("**/libjnidispatch.so")
        }
    }
}

dependencies {
    // Platform flavor: read-path SDK. Re-exports core via :platform-sdk-jvm,
    // whose dash-sdk-native.properties pins the native lib to rs_sdk_ffi.
    api(project(":platform-sdk-jvm"))
    implementation(libs.kotlinx.coroutines.android)

    // JNA AAR for Android (contains the jnidispatch.so JNA needs at runtime)
    implementation("net.java.dev.jna:jna:5.14.0@aar")

    androidTestImplementation(libs.androidx.test.core)
}
