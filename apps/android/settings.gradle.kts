pluginManagement {
    repositories {
        // 国内直连 Google maven / Maven Central 不稳，走腾讯云 Gradle 插件镜像
        maven { url = uri("https://mirrors.cloud.tencent.com/gradle/") }
        google()
        mavenCentral()
        gradlePluginPortal()
    }
}

dependencyResolutionManagement {
    repositoriesMode.set(RepositoriesMode.FAIL_ON_PROJECT_REPOS)
    repositories {
        maven { url = uri("https://mirrors.cloud.tencent.com/gradle/") }
        google()
        mavenCentral()
    }
}

rootProject.name = "qingjian"
include(":app")
