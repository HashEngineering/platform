apply(plugin = "org.jetbrains.kotlin.jvm")
apply(plugin = "application")

configure<org.gradle.api.plugins.JavaApplication> {
    mainClass.set("org.dash.sdk.console.DpnsSearchKt")
    // Auto-point JNA at the Rust release output so the native lib is found
    // without any extra setup. Run build_local.sh first.
    applicationDefaultJvmArgs = listOf(
        "-Djna.library.path=${rootProject.file("../../target/release").absolutePath}"
    )
}

dependencies {
    "implementation"(project(":sdk-jvm"))
}
