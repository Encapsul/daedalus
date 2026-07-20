# PHP Applications

Package PHP applications into self-extracting binaries. x.bin supports PHP apps using Composer, including Symfony, Laravel, and WordPress.

## Detection

x.bin detects PHP applications by looking for `composer.json` in the project root.

## Supported Frameworks

### Laravel

```bash
# Laravel projects with artisan
xbin build my-laravel-app -o my-laravel-app.xbin
```

x.bin detects Laravel by the presence of the `artisan` file and uses it as the entry point.

### Symfony

```bash
# Symfony projects with symfony.lock
xbin build my-symfony-app -o my-symfony-app.xbin
```

x.bin detects Symfony by `symfony.lock` or `config/bundles.php` and uses `bin/console` as the entry point.

### WordPress

```bash
# WordPress with wp-config.php
xbin build my-wordpress-site -o my-wordpress-site.xbin
```

### Generic PHP Apps

```bash
# Apps with public/index.php or index.php
xbin build my-php-app -o my-php-app.xbin
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

# Build the .xbin
xbin build . -o my-php-app.xbin

# Run it
./my-php-app.xbin
```

## Notes

- PHP extensions required by your app must be available on the target system
- Composer `vendor/` directory is included automatically
- For WordPress, `wp-content/` plugins and themes are included
