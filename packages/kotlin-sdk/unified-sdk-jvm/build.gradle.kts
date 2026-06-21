apply(plugin = "org.jetbrains.kotlin.jvm")

configure<org.gradle.api.plugins.JavaPluginExtension> {
    sourceCompatibility = JavaVersion.VERSION_17
    targetCompatibility = JavaVersion.VERSION_17
}

tasks.withType<org.jetbrains.kotlin.gradle.tasks.KotlinCompile>().configureEach {
    compilerOptions { jvmTarget.set(org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17) }
}

dependencies {
    // Unified flavor = platform (read-path) + wallet/shielded. The unified native
    // library is a superset of the read-path symbols, so this re-exports the platform
    // flavor's read-path bindings (`api`) and adds unified-only bindings on top.
    //
    // The dash-sdk-native.properties resource in THIS module pins the native lib to
    // rs_unified_sdk_ffi. platform-sdk-jvm intentionally ships no such resource, so
    // there is exactly one on the unified classpath (no ambiguity).
    "api"(project(":platform-sdk-jvm"))
}
