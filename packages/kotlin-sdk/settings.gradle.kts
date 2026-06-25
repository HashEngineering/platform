pluginManagement {
    repositories {
        google()
        mavenCentral()
        gradlePluginPortal()
    }
}

dependencyResolutionManagement {
    repositoriesMode.set(RepositoriesMode.FAIL_ON_PROJECT_REPOS)
    repositories {
        google()
        mavenCentral()
    }
}

rootProject.name = "KotlinDashSDK"

// Platform flavor → rs-sdk-ffi (read-path). Holds the read-path SDK code.
include(":platform-sdk-jvm")
include(":platform-sdk-android")

// Unified flavor → rs-unified-sdk-ffi (full SDK + wallet + shielded).
// Builds on the platform flavor (unified is a superset of the read-path symbols).
include(":unified-sdk-jvm")
include(":unified-sdk-android")

include(":console")

// Demo Android app: DPNS "Username Search" (Jetpack Compose) over :platform-sdk-android.
include(":username-search-app")
