apply(plugin = "org.jetbrains.kotlin.jvm")
apply(plugin = "application")

configure<org.gradle.api.plugins.JavaApplication> {
    mainClass.set("org.dash.sdk.console.DpnsSearchKt")
    // Auto-point JNA at the Rust release output so the native lib is found
    // without any extra setup. Run build_platform_local.sh first.
    applicationDefaultJvmArgs = listOf(
        "-Djna.library.path=${rootProject.file("../../target/release").absolutePath}"
    )
}

dependencies {
    "implementation"(project(":platform-sdk-jvm"))
}

// The `run` task launches DpnsSearch (the configured application mainClass).
// Add a sibling task to launch the GetIdentity console program.
tasks.register<JavaExec>("runGetIdentity") {
    group = "application"
    description = "Run the GetIdentity console program."
    mainClass.set("org.dash.sdk.console.GetIdentityKt")
    classpath = the<SourceSetContainer>()["main"].runtimeClasspath
    jvmArgs = listOf(
        "-Djna.library.path=${rootProject.file("../../target/release").absolutePath}"
    )
    standardInput = System.`in`
}
