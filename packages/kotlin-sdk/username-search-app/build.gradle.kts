plugins {
    alias(libs.plugins.android.application)
    alias(libs.plugins.kotlin.android)
    alias(libs.plugins.kotlin.compose)
}

android {
    namespace = "org.dash.sdk.usernamesearch"
    compileSdk = 35

    defaultConfig {
        applicationId = "org.dash.sdk.usernamesearch"
        minSdk = 24
        targetSdk = 35
        versionCode = 1
        versionName = "1.0"

        // Match the SDK's 64-bit-only native libraries (librs_sdk_ffi.so ships only for
        // arm64-v8a and x86_64; 32-bit ABIs are intentionally unsupported).
        ndk {
            abiFilters += listOf("arm64-v8a", "x86_64")
        }
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

    buildFeatures {
        compose = true
    }
}

dependencies {
    // The Dash Platform read-path SDK (Android delivery layer). This transitively brings
    // the pure-JVM SDK code (:platform-sdk-jvm) and packages librs_sdk_ffi.so into this
    // app's APK under the matching ABIs.
    //
    // :platform-sdk-jvm exposes the plain JNA *jar* (`api`), needed for host unit tests. On
    // Android we need the JNA *@aar* instead (it ships libjnidispatch.so). Both share the
    // coordinate net.java.dev.jna:jna, so leaving both in collides at dex time
    // ("Duplicate class com.sun.jna.*"). Exclude the transitive jna and add the @aar once.
    implementation(project(":platform-sdk-android")) {
        exclude(group = "net.java.dev.jna", module = "jna")
    }
    implementation("net.java.dev.jna:jna:5.14.0@aar")

    implementation(libs.androidx.core.ktx)
    implementation(libs.androidx.activity.compose)
    implementation(libs.androidx.lifecycle.viewmodel.compose)
    implementation(libs.androidx.lifecycle.runtime.compose)
    implementation(libs.kotlinx.coroutines.android)

    val composeBom = platform(libs.compose.bom)
    implementation(composeBom)
    implementation(libs.compose.ui)
    implementation(libs.compose.ui.tooling.preview)
    implementation(libs.compose.material3)
    debugImplementation(libs.compose.ui.tooling)
}
