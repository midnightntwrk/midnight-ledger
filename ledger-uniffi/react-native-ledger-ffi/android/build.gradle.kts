
plugins {
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
    id("maven-publish")
}



android {
    namespace = "com.midnight.ledgerffi"
    compileSdk = 34

    defaultConfig {
        minSdk = 21
        targetSdk = 34
        testInstrumentationRunner = "androidx.test.runner.AndroidJUnitRunner"
    }

    buildTypes {
        debug {
            isMinifyEnabled = false
        }
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
    
    // Ensure compatibility with consuming apps
    publishing {
        singleVariant("release") {
            withSourcesJar()
        }
    }
    
    

    sourceSets {
        named("main") {
            kotlin.srcDirs("src/main/kotlin")
            resources.srcDirs("src/main/resources")
        }
        named("test") {
            kotlin.srcDirs("src/test/kotlin")
        }
    }

    lint {
        abortOnError = false
    }
}

dependencies {
    implementation("org.jetbrains.kotlin:kotlin-stdlib:1.9.10")
    implementation("androidx.core:core-ktx:1.12.0")
    implementation("androidx.appcompat:appcompat:1.6.1")

    // React Native dependencies
    implementation("com.facebook.react:react-android:0.79.6")

    // JNA for native library loading
    implementation("net.java.dev.jna:jna:5.13.0")

    // Testing dependencies
    testImplementation("junit:junit:4.13.2")

    androidTestImplementation("androidx.test.ext:junit:1.1.5")
    androidTestImplementation("androidx.test.espresso:espresso-core:3.5.1")
}

// React Native library configuration
afterEvaluate {
    publishing {
        publications {
            create<MavenPublication>("release") {
                from(components["release"])
                groupId = "com.midnight.ledgerffi"
                artifactId = "react-native-ledger-ffi"
                version = "1.0.0"
            }
        }
    }
}
