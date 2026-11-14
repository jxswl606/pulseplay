// The app module: how PulsePlay itself is compiled into an APK.
plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.compose")
    id("org.jetbrains.kotlin.plugin.parcelize")   // Chiaki.kt's @Parcelize types
}

android {
    namespace = "dev.pulseplay"
    compileSdk = 35
    // NDK version chiaki-ng's engine is built/tested with.
    ndkVersion = "28.2.13676358"

    defaultConfig {
        applicationId = "dev.pulseplay"
        minSdk = 26          // Android 8.0: floor for reliable MediaCodec hardware video decode.
        targetSdk = 35
        versionCode = 1
        versionName = "0.1.0-m1"

        // Build only arm64 for now (what real handhelds/phones use) to keep CI fast.
        ndk { abiFilters += "arm64-v8a" }

        // Flags passed to chiaki-ng's root CMake: build only the lib + Android JNI,
        // nothing desktop. Mirrors chiaki-ng/android/app/build.gradle.
        externalNativeBuild {
            cmake {
                arguments += listOf(
                    "-DCHIAKI_ENABLE_TESTS=OFF",
                    "-DCHIAKI_ENABLE_CLI=OFF",
                    "-DCHIAKI_ENABLE_GUI=OFF",
                    "-DCHIAKI_ENABLE_SETSU=OFF",
                    "-DCHIAKI_ENABLE_ANDROID=ON",
                    "-DCHIAKI_ENABLE_FFMPEG_DECODER=OFF",
                    "-DCHIAKI_ENABLE_PI_DECODER=OFF",
                    "-DCHIAKI_LIB_ENABLE_OPUS=OFF",
                    "-DCHIAKI_LIB_OPENSSL_EXTERNAL_PROJECT=ON",
                    "-DCHIAKI_LIB_JSONC_EXTERNAL_PROJECT=ON",
                    "-DCHIAKI_LIB_MINIUPNPC_EXTERNAL_PROJECT=ON",
                    "-DCHIAKI_ENABLE_STEAM_SHORTCUT=OFF",
                    "-DCHIAKI_ENABLE_STEAMDECK_NATIVE=OFF",
                    "-DANDROID_STL=c++_shared",
                    "-DANDROID_SUPPORT_FLEXIBLE_PAGE_SIZES=ON",
                    "-DCMAKE_SHARED_LINKER_FLAGS=-Wl,-z,max-page-size=16384 -Wl,-z,common-page-size=16384",
                )
            }
        }
    }

    buildTypes {
        // Debug build = the test APK you install. No signing config needed for debug.
        getByName("debug") {
            isMinifyEnabled = false
            applicationIdSuffix = ".debug"
            versionNameSuffix = "-debug"
        }
        getByName("release") {
            isMinifyEnabled = false   // turned on later, once the engine is integrated
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions {
        jvmTarget = "17"
        freeCompilerArgs += "-opt-in=kotlin.ExperimentalUnsignedTypes"
    }
    buildFeatures {
        compose = true
        prefab = true   // exposes the oboe audio library's CMake config to the native build
    }

    // Point the native build at chiaki-ng's root CMake (vendored as a submodule).
    externalNativeBuild {
        cmake {
            path = file("../engine/chiaki-ng/CMakeLists.txt")
            version = "3.22.1"
        }
    }
}

dependencies {
    implementation(platform("androidx.compose:compose-bom:2024.10.01"))
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.material3:material3")
    implementation("androidx.compose.ui:ui-tooling-preview")
    implementation("androidx.activity:activity-compose:1.9.3")
    implementation("androidx.core:core-ktx:1.13.1")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.8.1")
    // Audio output for the native engine (chiaki-jni links oboe via prefab).
    implementation("com.google.oboe:oboe:1.9.3")
    debugImplementation("androidx.compose.ui:ui-tooling")
}
