"""Java runtime detection and embedding.

Supports Maven (pom.xml) and Gradle (build.gradle / build.gradle.kts).
Detection: pom.xml or build.gradle present.
Entry point: java -jar <built-jar> or java -cp <classpath> <mainClass>.
"""

from __future__ import annotations

import shutil
import xml.etree.ElementTree as ET
from pathlib import Path

from . import Runtime, RuntimePlan


class JavaRuntime(Runtime):
    name = "java"

    def detect(self, app_dir: Path) -> RuntimePlan | None:
        if (app_dir / "pom.xml").is_file():
            return _detect_maven(app_dir)
        if (app_dir / "build.gradle").is_file():
            return _detect_gradle(app_dir)
        if (app_dir / "build.gradle.kts").is_file():
            return _detect_gradle(app_dir)
        return None


def _find_java() -> Path:
    """Locate the java binary."""
    java = shutil.which("java")
    if not java:
        raise ValueError(
            "Java project detected but no java on PATH. "
            "Install a JDK (e.g. openjdk-21-jdk)."
        )
    return Path(java).resolve()


def _java_home(java_bin: Path) -> Path:
    """Resolve JAVA_HOME from the java binary path."""
    # Typical layout: $JAVA_HOME/bin/java
    return java_bin.parent.parent


def _find_jar(app_dir: Path, build_system: str) -> Path | None:
    """Find the built JAR file."""
    if build_system == "maven":
        target = app_dir / "target"
        if target.is_dir():
            for jar in sorted(
                target.glob("*.jar"), key=lambda p: p.stat().st_mtime, reverse=True
            ):
                if not jar.name.endswith("-sources.jar") and not jar.name.endswith(
                    "-javadoc.jar"
                ):
                    return jar
    elif build_system == "gradle":
        libs = app_dir / "build" / "libs"
        if libs.is_dir():
            for jar in sorted(
                libs.glob("*.jar"), key=lambda p: p.stat().st_mtime, reverse=True
            ):
                if not jar.name.endswith("-sources.jar"):
                    return jar
    return None


def _parse_main_class_pom(app_dir: Path) -> str | None:
    """Extract mainClass from pom.xml maven-jar-plugin or exec-maven-plugin."""
    try:
        tree = ET.parse(app_dir / "pom.xml")
        root = tree.getroot()
        ns = ""
        if root.tag.startswith("{"):
            ns = root.tag.split("}")[0] + "}"

        # Check maven-jar-plugin <mainClass>
        for plugin in root.iter(f"{ns}plugin"):
            artifact = plugin.findtext(f"{ns}artifactId", "")
            if artifact == "maven-jar-plugin":
                config = plugin.find(f"{ns}configuration")
                if config is not None:
                    mc = config.findtext(f"{ns}mainClass")
                    if mc:
                        return mc

        # Check exec-maven-plugin <mainClass>
        for plugin in root.iter(f"{ns}plugin"):
            artifact = plugin.findtext(f"{ns}artifactId", "")
            if artifact == "exec-maven-plugin":
                config = plugin.find(f"{ns}configuration")
                if config is not None:
                    mc = config.findtext(f"{ns}mainClass")
                    if mc:
                        return mc

        # Check spring-boot-maven-plugin <mainClass>
        for plugin in root.iter(f"{ns}plugin"):
            artifact = plugin.findtext(f"{ns}artifactId", "")
            if "spring-boot" in artifact:
                config = plugin.find(f"{ns}configuration")
                if config is not None:
                    mc = config.findtext(f"{ns}mainClass")
                    if mc:
                        return mc

        # Check MANIFEST.MF in built JAR (fallback)
        jar = _find_jar(app_dir, "maven")
        if jar:
            return _main_class_from_jar_manifest(jar)

    except (ET.ParseError, OSError):
        pass
    return None


def _parse_main_class_gradle(app_dir: Path) -> str | None:
    """Extract mainClass from build.gradle or build.gradle.kts."""
    for name in ("build.gradle", "build.gradle.kts"):
        path = app_dir / name
        if not path.is_file():
            continue
        try:
            text = path.read_text()
            for line in text.splitlines():
                stripped = line.strip()
                # mainClass = "com.example.App"
                # mainClass.set("com.example.App")
                # application { mainClass.set("com.example.App") }
                if "mainClass" in stripped:
                    for part in stripped.split("="):
                        part = part.strip().strip('"').strip("'").rstrip(";")
                        if "." in part and part[0].isupper():
                            return part
        except OSError:
            pass

    # Fallback: check MANIFEST.MF in built JAR
    jar = _find_jar(app_dir, "gradle")
    if jar:
        return _main_class_from_jar_manifest(jar)
    return None


def _main_class_from_jar_manifest(jar: Path) -> str | None:
    """Read Main-Class from a JAR's MANIFEST.MF."""
    import zipfile

    try:
        with zipfile.ZipFile(jar) as zf:
            manifest = zf.read("META-INF/MANIFEST.MF").decode("utf-8", errors="replace")
            for line in manifest.splitlines():
                if line.lower().startswith("main-class:"):
                    return line.split(":", 1)[1].strip()
    except (zipfile.BadZipFile, KeyError, OSError):
        pass
    return None


def _detect_maven(app_dir: Path) -> RuntimePlan:
    java_bin = _find_java()
    main_class = _parse_main_class_pom(app_dir)
    jar = _find_jar(app_dir, "maven")

    if jar and main_class:
        entrypoint = ["/opt/java/bin/java", "-jar", f"/app/{jar.name}"]
    elif main_class:
        entrypoint = ["/opt/java/bin/java", main_class]
    elif jar:
        entrypoint = ["/opt/java/bin/java", "-jar", f"/app/{jar.name}"]
    else:
        raise ValueError(
            "Maven project detected (pom.xml) but no built JAR found in target/. "
            "Run 'mvn package' first."
        )

    return RuntimePlan(
        runtime="java",
        interpreter_host=java_bin,
        entrypoint=entrypoint,
        cwd="/app",
    )


def _detect_gradle(app_dir: Path) -> RuntimePlan:
    java_bin = _find_java()
    main_class = _parse_main_class_gradle(app_dir)
    jar = _find_jar(app_dir, "gradle")

    if jar and main_class:
        entrypoint = ["/opt/java/bin/java", "-jar", f"/app/{jar.name}"]
    elif main_class:
        entrypoint = ["/opt/java/bin/java", main_class]
    elif jar:
        entrypoint = ["/opt/java/bin/java", "-jar", f"/app/{jar.name}"]
    else:
        raise ValueError(
            "Gradle project detected (build.gradle) but no built JAR found in build/libs/. "
            "Run 'gradle build' first."
        )

    return RuntimePlan(
        runtime="java",
        interpreter_host=java_bin,
        entrypoint=entrypoint,
        cwd="/app",
    )
