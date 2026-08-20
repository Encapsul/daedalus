# /erebus-qa — Build + browser-test an erebus app

Uses gstack's headless browser (`$B`) to QA web apps packaged with erebus.

## Prerequisites
- erebus binary on PATH
- gstack installed (provides browser daemon)

## Steps
1. Build the app: `cargo build --release --bin erebus`
2. Package the web app: `erebus build ./web-app --gui --isolation sandbox -o test.erebus`
3. Run the binary: `./test.erebus &`
4. Browser test via gstack: `$B goto http://localhost:8080`
5. Screenshot + assertions: `$B screenshot`, `$B text`

## Notes
- The erebus binary self-extracts to `~/.cache/erebus/<hash>/rootfs/`
- The app runs on a random port — check stdout
- Use `--isolation none` for easier browser debugging
