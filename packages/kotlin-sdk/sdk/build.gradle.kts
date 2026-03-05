plugins {
    alias(libs.plugins.android.library)
    alias(libs.plugins.kotlin.android)
}

android {
    namespace = "org.dash.sdk"
    compileSdk = 35

    defaultConfig {
        minSdk = 24

        // ABI filters - match what build_android.sh produces
        ndk {
            abiFilters += listOf("arm64-v8a", "armeabi-v7a", "x86", "x86_64")
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
            val os = System.getProperty("os.name").lowercase()
            val libName = when {
                os.contains("mac") -> "librs_sdk_ffi.dylib"
                os.contains("linux") -> "librs_sdk_ffi.so"
                else -> "rs_sdk_ffi.dll"
            }
            // sdk/ → kotlin-sdk/ → packages/ → platform root → target/release
            val nativeLibDir = project.file("../../../target/release")
            if (nativeLibDir.resolve(libName).exists()) {
                test.systemProperty("dash.sdk.lib.dir", nativeLibDir.absolutePath)
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
    implementation(libs.kotlin.stdlib)
    implementation(libs.kotlinx.coroutines.core)
    implementation(libs.kotlinx.coroutines.android)

    // JNA (Java Native Access) — allows calling C functions by name without a
    // hand-written JNI wrapper, analogous to how Swift imports the C header directly.
    // Classifier must be declared inline; version catalog doesn't support @aar notation.
    implementation("net.java.dev.jna:jna:5.14.0@aar")

    testImplementation(libs.junit)
    // JNA plain JAR for local JVM unit tests (no @aar needed on desktop)
    testImplementation("net.java.dev.jna:jna:5.14.0")
    androidTestImplementation(libs.androidx.test.core)
}
