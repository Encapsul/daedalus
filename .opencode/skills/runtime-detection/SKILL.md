---
name: runtime-detection
description: Runtime detection and entrypoint resolution for all 11 supported runtimes
---

## What I do

I handle runtime detection and entrypoint resolution for x.bin. I understand how each runtime is detected and how entrypoints are built.

## Supported runtimes

1. **Python** - `python3 /app/app.py`
2. **Node.js** - `node /app/index.js`
3. **Deno** - `deno run /app/main.ts`
4. **Java** - `java -cp /app Main`
5. **Ruby** - `ruby /app/main.rb`
6. **.NET/C#** - `dotnet /app/App.dll`
7. **Go** - `/app/app` (compiled binary)
8. **PHP** - `php /app/index.php`
9. **Perl** - `perl /app/main.pl`
10. **Binary** - `/app/app` (pre-compiled)
11. **Hugo** - `hugo server` or `hugo`

## Detection logic

```rust
// xbin-core/src/detect.rs
pub fn detect_runtime(app_dir: &Path) -> Result<Runtime> {
    // Check file extensions
    // Check shebangs
    // Check package.json, requirements.txt, etc.
    // Return detected runtime
}
```

## Entrypoint resolution

```rust
// xbin-core/src/detect.rs
pub fn resolve_entrypoint(runtime: &Runtime, app_dir: &Path) -> Result<Vec<String>> {
    match runtime {
        Runtime::Python => Ok(vec!["python3".into(), "/app/app.py".into()]),
        Runtime::Node => Ok(vec!["node".into(), "/app/index.js".into()]),
        // ... etc
    }
}
```

## Key concepts

1. **Interpreter names are bare** (no `/`) so `execvp` finds them on PATH
2. **App paths are absolute** (start with `/`) so `make_resolve` maps them to `rootfs/<path>`
3. **Entrypoint resolution happens at runtime**, not build time
4. **Cache hit skips extraction** if hash matches

## Files to modify

- `xbin-core/src/detect.rs`: Detection logic
- `xbin-core/src/metadata.rs`: Runtime enum
- `stub/src/main.rs`: Entrypoint execution

## Testing

```bash
# Test runtime detection
cargo test --workspace

# Test specific runtime
cargo test --workspace python
cargo test --workspace node
```
