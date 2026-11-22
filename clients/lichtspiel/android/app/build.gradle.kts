plugins {
    id("com.android.application")
    // The Flutter Gradle Plugin must be applied after the Android and Kotlin Gradle plugins.
    id("dev.flutter.flutter-gradle-plugin")
}

// Release builds are signed with the key in LICHTSPIEL_KEYSTORE (a .jks file) when it is set, as the release
// workflows do (docs/development.md, "Releases"); without it with the debug key, so `flutter run --release`
// works. Only APKs signed with the same key can update an installed app.
val releaseKeystore = System.getenv("LICHTSPIEL_KEYSTORE")?.takeIf { it.isNotEmpty() && file(it).exists() }

android {
    namespace = "de.lichtspiel.lichtspiel"
    compileSdk = flutter.compileSdkVersion
    ndkVersion = flutter.ndkVersion

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }

    defaultConfig {
        // TODO: Specify your own unique Application ID (https://developer.android.com/studio/build/application-id.html).
        applicationId = "de.lichtspiel.lichtspiel"
        // You can update the following values to match your application needs.
        // For more information, see: https://flutter.dev/to/review-gradle-config.
        minSdk = flutter.minSdkVersion
        targetSdk = flutter.targetSdkVersion
        // Uses the version code from pubspec.yaml. When using split APKs, 1000 * ABI_VERSION
        // is added automatically by Flutter. (https://developer.android.com/studio/build/configure-apk-splits#configure-APK-versions)
        // You can force using the value of versionCode by specifying the `-P force-version-code-ignoring-abi=true`
        // flag during build.
        versionCode = flutter.versionCode
        versionName = flutter.versionName
    }

    signingConfigs {
        if (releaseKeystore != null) {
            create("release") {
                storeFile = file(releaseKeystore)
                storePassword = System.getenv("LICHTSPIEL_KEYSTORE_PASSWORD")
                keyAlias = System.getenv("LICHTSPIEL_KEY_ALIAS") ?: "lichtspiel"
                keyPassword = System.getenv("LICHTSPIEL_KEY_PASSWORD") ?: System.getenv("LICHTSPIEL_KEYSTORE_PASSWORD")
            }
        }
    }

    buildTypes {
        release {
            signingConfig = signingConfigs.getByName(if (releaseKeystore != null) "release" else "debug")
        }
    }
}

kotlin {
    compilerOptions {
        jvmTarget = org.jetbrains.kotlin.gradle.dsl.JvmTarget.JVM_17
    }
}

flutter {
    source = "../.."
}
