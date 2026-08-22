# PHP Applications

Package PHP applications into self-extracting binaries. daedalus supports PHP apps using Composer, including Symfony, Laravel, and WordPress.

## Detection

daedalus detects PHP applications by looking for `composer.json` in the project root.

## Supported Frameworks

### Laravel

```bash
# Laravel projects with artisan
daedalus build my-laravel-app -o my-laravel-app.ere
```

daedalus detects Laravel by the presence of the `artisan` file and uses it as the entry point.

### Symfony

```bash
# Symfony projects with symfony.lock
daedalus build my-symfony-app -o my-symfony-app.ere
```

daedalus detects Symfony by `symfony.lock` or `config/bundles.php` and uses `bin/console` as the entry point.

### WordPress

```bash
# WordPress with wp-config.php
daedalus build my-wordpress-site -o my-wordpress-site.ere
```

### Generic PHP Apps

```bash
# Apps with public/index.php or index.php
daedalus build my-php-app -o my-php-app.ere
```

## Requirements

- PHP 8.0+ installed and available on PATH
- Composer dependencies installed (`composer install`)

## Example

```bash
# Create a simple PHP app
mkdir my-php-app && cd my-php-app
cat > composer.json << 'EOF'
{
    "name": "my-app",
    "require": {
        "php": ">=8.1"
    }
}
EOF

cat > index.php << 'EOF'
<?php
echo "Hello from PHP " . PHP_VERSION . "\n";
EOF

# Build the .ere
daedalus build . -o my-php-app.ere

# Run it
./my-php-app.ere
```

## Notes

- PHP extensions required by your app must be available on the target system
- Composer `vendor/` directory is included automatically
- For WordPress, `wp-content/` plugins and themes are included
