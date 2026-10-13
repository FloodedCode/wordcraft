plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

android {
    namespace = "com.wordcraft.android"
    compileSdk = 34

    defaultConfig {
        applicationId = "com.wordcraft.android"
        minSdk = 26
        targetSdk = 34
        versionCode = 61
        versionName = "0.6.1-android"
    }

    signingConfigs {
        create("release") {
            val ksFile = file("release-key.jks")
            if (ksFile.exists()) {
                storeFile = ksFile
                storePassword = System.getenv("KEYSTORE_PASSWORD") ?: "wordcraft"
                keyAlias = System.getenv("KEY_ALIAS") ?: "wordcraft"
                keyPassword = System.getenv("KEY_PASSWORD") ?: "wordcraft"
            }
        }
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            val ksFile = file("release-key.jks")
            if (ksFile.exists()) {
                signingConfig = signingConfigs.getByName("release")
            }
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions {
        jvmTarget = "17"
    }
}

dependencies {
    implementation("androidx.webkit:webkit:1.10.0")
}
