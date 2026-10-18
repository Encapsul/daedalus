# Hub catalog

`catalog.json` lists packaged apps and recipes. `build.sh` rebuilds the
`buildable` entries into `dist/` (gitignored — do not commit `.de` files).

```bash
hub/build.sh            # all buildable apps
hub/build.sh hello-web  # one app
```

See [docs/src/guides/hub.md](../docs/src/guides/hub.md).
