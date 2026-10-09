plugins {
    id("com.android.application") version "8.5.2" apply false
    id("org.jetbrains.kotlin.android") version "1.9.24" apply false
}

val localBuildDir = File(System.getProperty("user.home"), ".gradle-builds/wordcraft-android")

allprojects {
    layout.buildDirectory.set(File(localBuildDir, project.name))
}
