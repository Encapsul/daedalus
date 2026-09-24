# The hub — one template per popular runtime

A community catalog of daedalus-packaged apps. The rule is deliberately
tight: **one canonical app per runtime**, not a platform. A dev hits the
frustration of their runtime ("pip is broken again"), sees the pattern
("one file, runs anywhere"), and reuses it.

## Layout

```
hub/
  catalog.json       # the catalog (one entry per app)
  catalog.schema.json
  build.sh           # rebuild buildable apps into hub/dist/
  dist/              # build output (gitignored)
```

## Adding an app

1. Add an entry to `hub/catalog.json`.
2. If the source lives in this repo, set `"buildable": true` and a `build`
   array; `hub/build.sh` will rebuild it. Otherwise document the canonical
   source layout under `recipe` and keep `"buildable": false`.

Entry fields: `id`, `name`, `runtime`, `template` (`application` /
`service` / `plugin`), `description`, optional `source`, `build` or
`recipe`, `run`, `port`, `buildable`.

```json
{
  "id": "my-app",
  "name": "My App",
  "runtime": "python",
  "template": "application",
  "description": "What it shows.",
  "source": "examples/my-app",
  "build": ["daedalus", "build", "./examples/my-app", "-o", "hub/dist/my-app.de"],
  "run": "./my-app.de",
  "port": 8080,
  "buildable": true
}
```

## Rebuilding

```bash
hub/build.sh            # all buildable apps
hub/build.sh my-app     # a single app
```

Output lands in `hub/dist/` (gitignored — artifacts are meant to be shipped
through `release.yml`/the registry, not committed).

## Verified apps in the catalog

- **hello-web** — Python stdlib HTTP, zero deps.
- **hello-node** — Node stdlib HTTP.
- **bottle-web** — Python Bottle with vendored `site-packages` (no pip/network).
- **clinic-agent** — offline Gemma AI for rural clinics (see
  [The 60-second demo](./demo-60s.md)).

Recipe-only rows cover Express, FastAPI, Spring Boot, Rails, Laravel and Go
(Gin) with the exact layout + build command to reproduce one file per
runtime.