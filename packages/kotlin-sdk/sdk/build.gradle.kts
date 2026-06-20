plugins {
    alias(libs.plugins.android.library)
    alias(libs.plugins.kotlin.android)
}

android {
    namespace = "org.dash.sdk"
    compileSdk = 35

    defaultConfig {
        minSdk = 24

        // ABI filters - match what build_android.sh produces.
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

    testOptions {
        unitTests.all { test ->
            // Point JVM unit tests at the locally-built native library.
            // Run build_local.sh (Mac/Linux) or build_local.bat (Windows) first.
            //
            // Library selection (default rs_sdk_ffi, opt-in rs_unified_sdk_ffi) is controlled
            // by the `dashNativeLib` Gradle property or `dash.sdk.native.lib` system property,
            // e.g. `./gradlew :sdk:testDebugUnitTest -PdashNativeLib=rs_unified_sdk_ffi`.
            val nativeLib = (project.findProperty("dashNativeLib") as String?)
                ?: System.getProperty("dash.sdk.native.lib")
                ?: "rs_sdk_ffi"
            val os = System.getProperty("os.name").lowercase()
            val libFile = when {
                os.contains("mac") -> "lib$nativeLib.dylib"
                os.contains("linux") -> "lib$nativeLib.so"
                else -> "$nativeLib.dll"
            }
            // sdk/ → kotlin-sdk/ → packages/ → platform root → target/release
            val nativeLibDir = project.file("../../../target/release")
            if (nativeLibDir.resolve(libFile).exists()) {
                test.systemProperty("dash.sdk.lib.dir", nativeLibDir.absolutePath)
                test.systemProperty("dash.sdk.native.lib", nativeLib)
            }
        }
    }

    kotlinOptions {
        jvmTarget = "17"
    }

    // Pre-built .so files from build_android.sh are placed in src/main/jniLibs
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
    api(project(":sdk-jvm"))
    implementation(libs.kotlinx.coroutines.android)

    // JNA AAR for Android (contains the jnidispatch.so JNA needs at runtime)
    implementation("net.java.dev.jna:jna:5.14.0@aar")

    testImplementation(libs.junit)
    // JNA plain JAR for local JVM unit tests (no @aar needed on desktop)
    testImplementation("net.java.dev.jna:jna:5.14.0")
    androidTestImplementation(libs.androidx.test.core)
}
