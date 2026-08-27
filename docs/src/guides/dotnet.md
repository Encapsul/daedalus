# Building a .NET / C# App

`daedalus` supports .NET apps. It detects a .NET project by the presence of a
`*.csproj` or `*.sln` file.

## Detection

| File | Strategy |
|------|----------|
| `*.csproj` | Reads `<OutputType>` (Exe or Library) |
| `publish/` dir | Uses pre-published output (self-contained or framework-dependent) |
| `bin/Release/net*/publish/` | Standard publish location |

## Prerequisites

- .NET SDK installed (`dotnet` on PATH)
- For published apps: `dotnet publish` before building

## Build

```bash
# Published app (recommended)
cd my-dotnet-app
dotnet publish -c Release
daedalus build . -o my-app.de

# Dev project (uses dotnet run, slower startup)
daedalus build . -o my-app.de
```

The builder:

1. detects the `dotnet` runtime and parses the `.csproj` for `OutputType`;
2. checks for a `publish/` directory (pre-built output);
3. embeds the `dotnet` interpreter and its shared libraries;
4. packages the published DLLs into the app layer;
5. compresses and assembles the `.de`.

## Published vs dev mode

### Published (recommended)

If `publish/` or `bin/Release/net*/publish/` exists, the launcher runs:

```bash
/opt/dotnet/dotnet /app/publish/MyApp.dll
```

This is faster and produces a smaller binary (no SDK needed at runtime).

### Dev mode (fallback)

If no publish directory exists, the launcher runs:

```bash
/opt/dotnet/dotnet run --project /app/MyApp.csproj
```

This requires the SDK in the runtime layer (larger binary, slower startup).

## Self-contained vs framework-dependent

- **Self-contained** (`dotnet publish --self-contained`): includes the .NET
  runtime in the output. Larger binary but no runtime dependency on the target.
- **Framework-dependent** (`dotnet publish`): requires .NET runtime on the target.
  Smaller binary but the runtime must match.

`daedalus` works with both. Self-contained is recommended for maximum portability.

## Environment variables

```bash
DOTNET_ENVIRONMENT=Production ./my-app.de
ASPNETCORE_URLS="http://0.0.0.0:9000" ./my-app.de
```

## Known limitations

- Only .NET 8+ is tested. Earlier versions may work but are not guaranteed.
- .NET class libraries (`<OutputType>Library</OutputType>`) cannot be packaged
  directly — they need an executable entry point.
- Blazor Server apps work. Blazor WebAssembly does not (requires browser).
- F# projects (`*.fsproj`) are not yet supported but should work once detected.
