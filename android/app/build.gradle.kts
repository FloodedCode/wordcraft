plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

android {
    namespace = "ai.storyteller.wordcraft"
    compileSdk = 35

    defaultConfig {
        applicationId = "ai.storyteller.wordcraft"
        minSdk = 26
        targetSdk = 35
        versionCode = 1
        versionName = "0.3.0"

        ndk {
            abiFilters += listOf("arm64-v8a", "x86_64")
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                "proguard-rules.pro",
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

    sourceSets {
        getByName("main") {
            jniLibs.srcDirs("src/main/jniLibs")
        }
    }

    packaging {
        jniLibs {
            // Keep native libs uncompressed in the APK so the linker can load them directly.
            useLegacyPackaging = true
        }
    }
}

dependencies {
    // GameActivity 4.4.x — IME / soft keyboard; version must match android-activity expectations.
    implementation("androidx.games:games-activity:4.4.0")
    implementation("androidx.appcompat:appcompat:1.7.0")
    implementation("androidx.activity:activity-ktx:1.9.2")
    // WindowCompat: immersive fullscreen + inset controller
    implementation("androidx.core:core-ktx:1.13.1")
}

val isWindows = System.getProperty("os.name").lowercase().contains("windows")
val wordcraftRoot = rootProject.projectDir.parentFile

val buildRustNative = tasks.register<Exec>("buildRustNative") {
    description = "Compiles Rust native binaries via cargo-ndk into src/main/jniLibs"
    group = "build"
    workingDir = wordcraftRoot

    val script = if (isWindows) {
        listOf("powershell", "-ExecutionPolicy", "Bypass", "-File", "android/build-native.ps1")
    } else {
        listOf("bash", "android/build-native.sh")
    }
    commandLine(script)
}

tasks.named("preBuild") {
    dependsOn(buildRustNative)
}
