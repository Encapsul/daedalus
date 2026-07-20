# Building a Ruby App

`xbin` supports Ruby apps with or without Bundler. It detects a Ruby project by
the presence of a `Gemfile` or a single `.rb` file.

## Detection

| File | Strategy |
|------|----------|
| `Gemfile` | Bundler — uses `vendor/bundle/` or `.bundle/config` for gem paths |
| Single `*.rb` | Embeds Ruby interpreter, no dependency resolution |

## Prerequisites

- Ruby installed (`ruby` on PATH)
- For Bundler projects: run `bundle install` before building

## Build

```bash
# Bundler project
xbin build ./my-ruby-app -o my-ruby-app.xbin

# Single file
xbin build ./my-script -o my-script.xbin
```

The builder:

1. detects the `ruby` runtime;
2. for Bundler projects, reads `GEM_PATH` from `.bundle/config` or `vendor/bundle/`;
3. embeds the `ruby` interpreter and its shared libraries;
4. packages gems into the app layer;
5. compresses and assembles the `.xbin`.

## Entrypoint detection

The builder looks for a main script in this order:

1. `main.rb`
2. `app.rb`
3. `server.rb`
4. `config.ru` (Rack)
5. `config/ru` (Rails convention)
6. `Rakefile`

If none found, defaults to `main.rb`.

## Bundler projects

```bash
cd my-ruby-app
bundle install --deployment    # installs to vendor/bundle
xbin build . -o my-app.xbin
```

The builder reads `BUNDLE_PATH` from `.bundle/config` and embeds the gem
directory. At runtime, `GEM_PATH` is set to point to the embedded gems.

## Rails apps

Rails projects are detected by the presence of `config/ru`. The builder uses
`config.ru` as the entrypoint with `config.ru` (Rack-based startup).

```bash
cd my-rails-app
bundle install --deployment
xbin build . -o my-rails-app.xbin
```

## Environment variables

```bash
RAILS_ENV=production ./my-rails-app.xbin
PORT=3000 ./my-rails-app.xbin
```

## Known limitations

- Only Ruby MRI is supported (not JRuby, TruffleRuby, or mruby).
- Native C extensions (gems with `.so` files) require their system dependencies
  to be available on the build machine. The ELF analyzer resolves these.
- Rails asset pipeline compilation must happen before `xbin build`.
