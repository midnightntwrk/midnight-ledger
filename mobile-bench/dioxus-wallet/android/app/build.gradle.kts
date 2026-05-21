import groovy.json.JsonSlurper

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

/**
 * Resolve the `rustls-platform-verifier-android` AAR's on-disk
 * Maven repo. The Rust crate ships the Kotlin/Java JNI shim as a
 * vendored AAR inside its cargo-registry source tree; without this
 * AAR on the classpath every TLS handshake fails with
 * "failed to call native verifier: Error". `cargo metadata` is the
 * official way to locate it — it knows where Cargo cached the crate
 * for the current registry hash.
 *
 * The `manifest_path` field for the crate points at its `Cargo.toml`;
 * the sibling `maven/` dir is the local repo. See the upstream README
 * (`rustls-platform-verifier-0.6.2/README.md` §"Gradle setup").
 */
fun rustlsPlatformVerifierMavenPath(): String {
    val walletManifest = file("../../Cargo.toml").absolutePath
    val output = providers.exec {
        commandLine(
            "cargo", "metadata",
            "--format-version", "1",
            "--filter-platform", "aarch64-linux-android",
            "--manifest-path", walletManifest,
        )
    }.standardOutput.asText.get()
    @Suppress("UNCHECKED_CAST")
    val root = JsonSlurper().parseText(output) as Map<String, Any>
    @Suppress("UNCHECKED_CAST")
    val packages = root["packages"] as List<Map<String, Any>>
    val pkg = packages.first { it["name"] == "rustls-platform-verifier-android" }
    val manifestFile = file(pkg["manifest_path"] as String)
    return File(manifestFile.parentFile, "maven").absolutePath
}

repositories {
    maven {
        url = uri(rustlsPlatformVerifierMavenPath())
        // Read the bundled `.pom` so Gradle picks up `packaging=aar`
        // and resolves the `.aar` instead of looking for a `.jar`.
        metadataSources {
            mavenPom()
            artifact()
        }
    }
}

android {
    namespace="io.iohk.midnight.wallet"
    compileSdk = 33
    defaultConfig {
        applicationId = "io.iohk.midnight.wallet"
        minSdk = 24
        targetSdk = 33
        versionCode = 1
        versionName = "1.0"
    }
    buildTypes {
        getByName("debug") {
            isDebuggable = true
            isJniDebuggable = true
            isMinifyEnabled = false
            packaging {
                jniLibs.keepDebugSymbols.add("*/arm64-v8a/*.so")
                jniLibs.keepDebugSymbols.add("*/armeabi-v7a/*.so")
                jniLibs.keepDebugSymbols.add("*/x86/*.so")
                jniLibs.keepDebugSymbols.add("*/x86_64/*.so")
            }
        }
        getByName("release") {
            isMinifyEnabled = true
             proguardFiles(
                *fileTree(".") { include("**/*.pro") }
                    .plus(getDefaultProguardFile("proguard-android-optimize.txt"))
                    .toList().toTypedArray()
            )
        }
    }
    kotlinOptions {
        jvmTarget = "1.8"
    }
    buildFeatures {
        buildConfig = true
    }
}

dependencies {
    implementation("androidx.webkit:webkit:1.6.1")
    implementation("androidx.appcompat:appcompat:1.6.1")
    implementation("com.google.android.material:material:1.8.0")
    // Kotlin/Java JNI shim for rustls-platform-verifier. Loaded from
    // the local Maven repo declared above. Version pinned to the AAR
    // that ships with rustls-platform-verifier 0.6.2.
    implementation("rustls:rustls-platform-verifier:0.1.1")
}
