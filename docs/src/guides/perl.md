# Perl Applications

Package Perl applications into self-extracting binaries. x.bin supports Perl apps using Makefile.PL or cpanfile.

## Detection

x.bin detects Perl applications by looking for `Makefile.PL` or `cpanfile` in the project root.

## Supported Patterns

### PSGI Applications

```bash
# Perl apps with app.pl (PSGI)
xbin build my-perl-app -o my-perl-app.xbin
```

### CGI Scripts

```bash
# Traditional CGI scripts
xbin build my-cgi-app -o my-cgi-app.xbin
```

### CLI Tools

```bash
# Perl CLI tools with bin/app
xbin build my-perl-tool -o my-perl-tool.xbin
```

## Requirements

- Perl 5.30+ installed and available on PATH
- Dependencies installed via `cpanm` or `cpan`

## Example

```bash
# Create a simple Perl app
mkdir my-perl-app && cd my-perl-app

cat > Makefile.PL << 'EOF'
use ExtUtils::MakeMaker;
WriteMakefile(
    NAME => 'My::App',
    VERSION_FROM => 'lib/My/App.pm',
    PREREQ_PM => {
        'Mojolicious' => '0',
    },
);
EOF

cat > app.psgi << 'EOF'
use My::App;
My::App->new->start;
EOF

# Build the .xbin
xbin build . -o my-perl-app.xbin

# Run it
./my-perl-app.xbin
```

## Notes

- Perl modules required by your app must be installed
- For PSGI apps, the entry point is typically `app.psgi`
- For CLI tools, the entry point is typically `bin/app`
