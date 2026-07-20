# Building a Java App

`xbin` supports Java apps via Maven and Gradle. It detects a Java project by the
presence of `pom.xml` (Maven) or `build.gradle` / `build.gradle.kts` (Gradle).

## Detection

| File | Build system | Entrypoint strategy |
|------|-------------|---------------------|
| `pom.xml` | Maven | `-jar target/*.jar` or `-cp` with `mainClass` |
| `build.gradle` | Gradle | `-jar build/libs/*.jar` or `-cp` with `mainClass` |

The builder parses `<mainClass>` from:
- `maven-jar-plugin` configuration
- `exec-maven-plugin` configuration
- `spring-boot-maven-plugin` configuration
- `MANIFEST.MF` inside the built JAR (fallback)
- `application { mainClass.set(...) }` in Gradle DSL

## Prerequisites

- A JDK installed (`java` on PATH)
- The app must be built before packaging:
  - Maven: `mvn package`
  - Gradle: `gradle build`

## Build

```bash
# Maven project
xbin build ./my-java-app -o my-java-app.xbin

# Gradle project
xbin build ./my-gradle-app -o my-gradle-app.xbin
```

The builder:

1. detects the `java` runtime and locates the JAR in `target/` or `build/libs/`;
2. parses the `mainClass` from build config or JAR manifest;
3. embeds the `java` interpreter and its shared libraries;
4. packages the JAR into the app layer;
5. compresses and assembles the `.xbin`.

## Entrypoint

The launcher runs:

```bash
/opt/java/bin/java -jar /app/my-app.jar
```

or if `mainClass` is known but no fat JAR exists:

```bash
/opt/java/bin/java -cp /app/classes com.example.Main
```

## Spring Boot

Spring Boot apps are detected via `spring-boot-maven-plugin` or
`spring-boot-gradle-plugin`. The builder reads the `mainClass` from the plugin
configuration.

```bash
cd my-spring-app
mvn package
xbin build . -o my-spring-app.xbin
./my-spring-app.xbin
```

## Environment variables

```bash
JAVA_OPTS="-Xmx512m" ./my-java-app.xbin
PORT=9000 ./my-java-app.xbin
```

## Known limitations

- The JDK is embedded as a runtime layer (not a JRE). Final binary is ~100-200MB
  depending on the JDK version. A JRE-only mode is planned.
- GraalVM native-image support is not yet implemented.
- Multi-module Gradle builds: only the main module's JAR is packaged.
