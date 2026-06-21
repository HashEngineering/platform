apply(plugin = "org.jetbrains.kotlin.jvm")

configure<org.gradle.api.plugins.JavaPluginExtension> {
    sourceCompatibility = JavaVersion.VERSION_17
    targetCompatibility = JavaVersion.VERSION_17
}

tasks.withType<org.jetbrains.kotlin.gradle.tasks.KotlinCompile>().configureEach {
    compilerOptions { jvmTarget.set(org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17) }
}

// Point host JVM unit tests at the locally-built native library.
// Run ../build_platform_local.sh (rs_sdk_ffi) or ../build_unified_local.sh (rs_unified_sdk_ffi) first.
//
// Library selection is controlled by the `dashNativeLib` Gradle property or
// `dash.sdk.native.lib` system property, e.g.
//   ./gradlew :platform-sdk-jvm:test -PdashNativeLib=rs_unified_sdk_ffi
tasks.withType<Test>().configureEach {
    val nativeLib = (project.findProperty("dashNativeLib") as String?)
        ?: System.getProperty("dash.sdk.native.lib")
        ?: "rs_sdk_ffi"
    val os = System.getProperty("os.name").lowercase()
    val libFile = when {
        os.contains("mac") -> "lib$nativeLib.dylib"
        os.contains("linux") -> "lib$nativeLib.so"
        else -> "$nativeLib.dll"
    }
    // platform-sdk-jvm/ → kotlin-sdk/ → packages/ → platform root → target/release
    val nativeLibDir = project.file("../../../target/release")
    if (nativeLibDir.resolve(libFile).exists()) {
        systemProperty("dash.sdk.lib.dir", nativeLibDir.absolutePath)
        systemProperty("dash.sdk.native.lib", nativeLib)
    }
}

dependencies {
    "implementation"(kotlin("stdlib"))
    "api"("org.jetbrains.kotlinx:kotlinx-coroutines-core:1.9.0")
    "api"("net.java.dev.jna:jna:5.14.0")

    "testImplementation"("junit:junit:4.13.2")
}
