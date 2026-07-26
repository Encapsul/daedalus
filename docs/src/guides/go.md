# Go Applications

Package Go applications into self-extracting binaries. x.bin builds your Go code into a static binary and packages it.

## Detection

x.bin detects Go applications by looking for `go.mod` in the project root.

## How It Works

1. Detects `go.mod` in the project directory
2. Builds the Go binary using `go build`
3. Packages the static binary into the .xbin format
4. No interpreter needed at runtime — pure native execution

## Requirements

- Go 1.21+ installed and available on PATH
- `go.mod` file in the project root

## Example

```bash
# Create a simple Go app
mkdir my-go-app && cd my-go-app

go mod init my-go-app

cat > main.go << 'EOF'
package main

import (
    "fmt"
    "net/http"
)

func main() {
    http.HandleFunc("/", func(w http.ResponseWriter, r *http.Request) {
        fmt.Fprintf(w, "Hello from Go!")
    })
    http.ListenAndServe(":8080", nil)
}
EOF

# Build the .xbin
xbin build . -o my-go-app.xbin

# Run it
./my-go-app.xbin
```

## Cross-Compilation

Go has excellent cross-compilation support. You can build for different architectures:

```bash
# Build for Linux aarch64 from x86_64
GOOS=linux GOARCH=arm64 xbin build . -o my-go-app-arm64.xbin

# Build for macOS from Linux
GOOS=darwin GOARCH=arm64 xbin build . -o my-go-app-macos.xbin
```

## Notes

- Go produces static binaries, so no runtime dependencies are needed
- CGO_ENABLED=0 is recommended for fully static builds
- The resulting .xbin will be larger than the Go binary alone due to the launcher
