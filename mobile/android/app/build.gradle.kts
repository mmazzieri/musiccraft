import org.jetbrains.kotlin.gradle.dsl.JvmTarget

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.compose")
}

// The library the app shares with the desktop, built by cargo-ndk from crates/ffi.
val rustLibs = layout.buildDirectory.dir("rust/jniLibs")
val repository = rootDir.resolve("../..")

val buildRust by tasks.registering(Exec::class) {
    description = "Builds musiccraft-ffi for the phone's processors."
    workingDir = repository
    val out = rustLibs.get().asFile.absolutePath
    commandLine(
        "cargo", "ndk", "--platform", "26", "-t", "arm64-v8a", "-t", "x86_64", "-o", out,
        "build", "--release", "-p", "musiccraft-ffi",
    )
    inputs.dir(repository.resolve("crates/ffi"))
    inputs.dir(repository.resolve("crates/library"))
    inputs.file(repository.resolve("Cargo.lock"))
    outputs.dir(out)
}

android {
    namespace = "it.mauromazzieri.musiccraft"
    compileSdk = 35

    defaultConfig {
        applicationId = "it.mauromazzieri.musiccraft"
        minSdk = 26
        targetSdk = 35
        versionCode = 1
        versionName = "0.1.0"
        ndk { abiFilters += listOf("arm64-v8a", "x86_64") }
    }

    buildTypes {
        release {
            isMinifyEnabled = false
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    buildFeatures { compose = true }

    sourceSets {
        getByName("main") { jniLibs.srcDir(rustLibs) }
    }
}

kotlin {
    compilerOptions { jvmTarget.set(JvmTarget.JVM_17) }
}

tasks.named("preBuild") { dependsOn(buildRust) }

dependencies {
    val compose = platform("androidx.compose:compose-bom:2025.05.01")
    implementation(compose)
    implementation("androidx.core:core-ktx:1.16.0")
    implementation("androidx.activity:activity-compose:1.10.1")
    implementation("androidx.lifecycle:lifecycle-runtime-compose:2.9.0")
    implementation("androidx.compose.material3:material3")
    implementation("androidx.compose.material:material-icons-core")
    implementation("androidx.media3:media3-exoplayer:1.6.1")
    implementation("androidx.media3:media3-session:1.6.1")
}
