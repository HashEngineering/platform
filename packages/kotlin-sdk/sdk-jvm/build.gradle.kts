apply(plugin = "org.jetbrains.kotlin.jvm")

configure<org.gradle.api.plugins.JavaPluginExtension> {
    sourceCompatibility = JavaVersion.VERSION_17
    targetCompatibility = JavaVersion.VERSION_17
}

tasks.withType<org.jetbrains.kotlin.gradle.tasks.KotlinCompile>().configureEach {
    compilerOptions { jvmTarget.set(org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17) }
}

dependencies {
    "implementation"(kotlin("stdlib"))
    "api"("org.jetbrains.kotlinx:kotlinx-coroutines-core:1.9.0")
    "api"("net.java.dev.jna:jna:5.14.0")
}
