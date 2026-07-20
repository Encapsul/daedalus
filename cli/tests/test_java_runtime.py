"""Unit tests for Java runtime detection."""

from unittest.mock import patch

import pytest

from xbin.runtimes.java import JavaRuntime


@pytest.fixture
def java_runtime():
    return JavaRuntime()


class TestJavaMaven:
    def test_detect_pom_xml(self, java_runtime, tmp_path):
        (tmp_path / "pom.xml").write_text(
            "<project><groupId>com.example</groupId></project>"
        )
        jar_dir = tmp_path / "target"
        jar_dir.mkdir()
        (jar_dir / "app-1.0.jar").write_bytes(b"fake jar")
        with patch("xbin.runtimes.java.shutil.which", return_value="/usr/bin/java"):
            plan = java_runtime.detect(tmp_path)
        assert plan is not None
        assert plan.runtime == "java"

    def test_no_detect_without_pom(self, java_runtime, tmp_path):
        (tmp_path / "Main.java").write_text("public class Main {}")
        plan = java_runtime.detect(tmp_path)
        assert plan is None

    def test_detect_no_java_on_path(self, java_runtime, tmp_path):
        (tmp_path / "pom.xml").write_text("<project></project>")
        with (
            patch("xbin.runtimes.java.shutil.which", return_value=None),
            pytest.raises(ValueError, match="no java on PATH"),
        ):
            java_runtime.detect(tmp_path)

    def test_maven_jar_plugin_main_class(self, java_runtime, tmp_path):
        pom = (
            "<project><build><plugins>"
            "<plugin><artifactId>maven-jar-plugin</artifactId>"
            "<configuration><mainClass>com.example.App</mainClass></configuration>"
            "</plugin></plugins></project>"
        )
        (tmp_path / "pom.xml").write_text(pom)
        jar_dir = tmp_path / "target"
        jar_dir.mkdir()
        (jar_dir / "app-1.0.jar").write_bytes(b"fake jar")
        with patch("xbin.runtimes.java.shutil.which", return_value="/usr/bin/java"):
            plan = java_runtime.detect(tmp_path)
        assert plan is not None
        assert "com.example.App" in plan.entrypoint or "-jar" in " ".join(
            plan.entrypoint
        )

    def test_maven_requires_jar(self, java_runtime, tmp_path):
        (tmp_path / "pom.xml").write_text("<project></project>")
        with (
            patch("xbin.runtimes.java.shutil.which", return_value="/usr/bin/java"),
            pytest.raises(ValueError, match="no built JAR"),
        ):
            java_runtime.detect(tmp_path)

    def test_maven_entrypoint_uses_jar(self, java_runtime, tmp_path):
        (tmp_path / "pom.xml").write_text("<project></project>")
        jar_dir = tmp_path / "target"
        jar_dir.mkdir()
        (jar_dir / "myapp-2.0.jar").write_bytes(b"fake jar")
        with patch("xbin.runtimes.java.shutil.which", return_value="/usr/bin/java"):
            plan = java_runtime.detect(tmp_path)
        assert any("-jar" in part for part in plan.entrypoint)


class TestJavaGradle:
    def test_detect_build_gradle(self, java_runtime, tmp_path):
        (tmp_path / "build.gradle").write_text("plugins { id 'java' }")
        libs = tmp_path / "build" / "libs"
        libs.mkdir(parents=True)
        (libs / "app.jar").write_bytes(b"fake jar")
        with patch("xbin.runtimes.java.shutil.which", return_value="/usr/bin/java"):
            plan = java_runtime.detect(tmp_path)
        assert plan is not None
        assert plan.runtime == "java"

    def test_detect_build_gradle_kts(self, java_runtime, tmp_path):
        (tmp_path / "build.gradle.kts").write_text("plugins { java }")
        libs = tmp_path / "build" / "libs"
        libs.mkdir(parents=True)
        (libs / "app.jar").write_bytes(b"fake jar")
        with patch("xbin.runtimes.java.shutil.which", return_value="/usr/bin/java"):
            plan = java_runtime.detect(tmp_path)
        assert plan is not None

    def test_gradle_requires_jar(self, java_runtime, tmp_path):
        (tmp_path / "build.gradle").write_text("plugins { id 'java' }")
        with (
            patch("xbin.runtimes.java.shutil.which", return_value="/usr/bin/java"),
            pytest.raises(ValueError, match="no built JAR"),
        ):
            java_runtime.detect(tmp_path)

    def test_gradle_main_class(self, java_runtime, tmp_path):
        (tmp_path / "build.gradle").write_text('mainClass = "MyApp"')
        libs = tmp_path / "build" / "libs"
        libs.mkdir(parents=True)
        (libs / "app.jar").write_bytes(b"fake jar")
        with patch("xbin.runtimes.java.shutil.which", return_value="/usr/bin/java"):
            plan = java_runtime.detect(tmp_path)
        assert plan is not None
        assert any("-jar" in part for part in plan.entrypoint)


class TestJavaMisc:
    def test_supports_cross_false(self, java_runtime):
        assert java_runtime.supports_cross() is False
