plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
}

android {
    namespace = "com.perry.app"
    compileSdk = 35

    defaultConfig {
        applicationId = "com.perry.template"
        minSdk = 24
        targetSdk = 35
        versionCode = 1
        versionName = "1.0"

        ndk {
            abiFilters += "arm64-v8a"
        }
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

    kotlinOptions {
        jvmTarget = "17"
    }

    sourceSets {
        getByName("main") {
            jniLibs.srcDirs("src/main/jniLibs")
        }
    }
}

dependencies {
    implementation("androidx.core:core-ktx:1.12.0")
    // MediaSessionCompat — surfaces lock-screen / Bluetooth / Wear OS
    // metadata + transport callbacks. Used by `perry/media`'s
    // `setNowPlaying`.
    implementation("androidx.media:media:1.7.0")
    // perry/ui MapView (#517). Google Maps SDK for Android. The user
    // supplies an API key via the AndroidManifest.xml meta-data entry
    // (resolves the `__YOUR_GOOGLE_MAPS_API_KEY__` placeholder there).
    implementation("com.google.android.gms:play-services-maps:18.2.0")
    // perry/background (#538) — WorkManager-backed deferred / periodic
    // tasks. PerryBackgroundWorker.kt extends `androidx.work.Worker`.
    implementation("androidx.work:work-runtime-ktx:2.9.0")
}
