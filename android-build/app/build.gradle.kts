plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    // Firebase Cloud Messaging (#95). Requires `google-services.json` in
    // `app/` — a placeholder ships in this repo for build-time only;
    // overlay your real project's file before deploying for FCM to work.
    id("com.google.gms.google-services")
}

android {
    namespace = "com.perry.app"
    compileSdk = 35

    defaultConfig {
        applicationId = "com.skelpo.pickcolor"
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
    // Firebase BoM pins all Firebase libs to a tested-together set of
    // versions; messaging-ktx 23.x is the line that ships with BoM 33.x.
    implementation(platform("com.google.firebase:firebase-bom:33.5.1"))
    implementation("com.google.firebase:firebase-messaging-ktx")
    // perry/background (#538) — WorkManager-backed deferred / periodic
    // tasks. PerryBackgroundWorker.kt extends `androidx.work.Worker`.
    implementation("androidx.work:work-runtime-ktx:2.9.0")
}
