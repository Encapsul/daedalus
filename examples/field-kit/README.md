# Field kit

Stdlib-only Python example that packages into one `.de` and exercises
**adjacent** uses of daedalus - USB hand-off, classroom labs, air-gapped
incident kits - without pretending a model is inside the file.

```bash
daedalus build ./examples/field-kit -o field-kit.de
./field-kit.de domains
./field-kit.de handoff
./field-kit.de inventory
```

See [Adjacent uses](../../docs/src/concepts/adjacent-uses.md).
