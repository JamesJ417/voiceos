plugins {
    id("com.android.application")
}

val gatewayUrl = providers.gradleProperty("AIOS_SERVER_URL")
    .orElse("http://10.0.2.2:8787")
    .get()

android {
    namespace = "dev.voiceos.client"
    compileSdk = 36

    defaultConfig {
        applicationId = "dev.voiceos.client"
        minSdk = 31
        targetSdk = 36
        versionCode = 4
        versionName = "0.4.0"

        buildConfigField("String", "GATEWAY_BASE_URL", "\"$gatewayUrl\"")
    }

    buildFeatures {
        buildConfig = true
    }

    buildTypes {
        debug {
            manifestPlaceholders["usesCleartextTraffic"] = "true"
        }
        release {
            manifestPlaceholders["usesCleartextTraffic"] = "false"
            isMinifyEnabled = true
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
}

dependencies {
    testImplementation("junit:junit:4.13.2")
}
