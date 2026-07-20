"""Unit tests for PHP runtime detection."""

import pytest

from xbin.runtimes.php import PHPRuntime


@pytest.fixture
def php_runtime():
    return PHPRuntime()


class TestPHPRuntime:
    def test_detect_with_composer_json(self, php_runtime, tmp_path):
        composer = tmp_path / "composer.json"
        composer.write_text('{"name": "test/app"}')
        index = tmp_path / "index.php"
        index.write_text("<?php echo 'hello'; ?>")

        plan = php_runtime.detect(tmp_path)
        assert plan is not None
        assert plan.runtime == "php"

    def test_no_detect_without_composer(self, php_runtime, tmp_path):
        index = tmp_path / "index.php"
        index.write_text("<?php echo 'hello'; ?>")

        plan = php_runtime.detect(tmp_path)
        assert plan is None

    def test_laravel_detection(self, php_runtime, tmp_path):
        composer = tmp_path / "composer.json"
        composer.write_text('{"name": "test/laravel-app"}')
        artisan = tmp_path / "artisan"
        artisan.write_text("#!/usr/bin/env php")

        plan = php_runtime.detect(tmp_path)
        assert plan is not None
        assert "artisan" in plan.entrypoint[1]

    def test_symfony_detection(self, php_runtime, tmp_path):
        composer = tmp_path / "composer.json"
        composer.write_text('{"name": "test/symfony-app"}')
        symfony_lock = tmp_path / "symfony.lock"
        symfony_lock.write_text("{}")
        bin_console = tmp_path / "bin"
        bin_console.mkdir()
        console = bin_console / "console"
        console.write_text("#!/usr/bin/env php")

        plan = php_runtime.detect(tmp_path)
        assert plan is not None
        assert "bin/console" in plan.entrypoint[1]

    def test_wordpress_detection(self, php_runtime, tmp_path):
        composer = tmp_path / "composer.json"
        composer.write_text('{"name": "test/wordpress"}')
        wp_config = tmp_path / "wp-config.php"
        wp_config.write_text("<?php // WordPress config")

        plan = php_runtime.detect(tmp_path)
        assert plan is not None

    def test_supports_cross_false(self, php_runtime):
        assert php_runtime.supports_cross() is False
