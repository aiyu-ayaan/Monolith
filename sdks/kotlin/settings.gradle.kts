pluginManagement {
    repositories {
        google()
        mavenCentral()
        gradlePluginPortal()
    }
}

dependencyResolutionManagement {
    repositories {
        google()
        mavenCentral()
    }
}

rootProject.name = "monolith-kotlin"
include(":monolith-core")
include(":monolith-android")
include(":monolith-compose")
