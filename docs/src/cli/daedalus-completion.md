# `daedalus completion`

Generate shell completion scripts.

```bash
daedalus completion <SHELL>
```

Supported shells: `bash`, `zsh`, `fish`, `elvish`, `powershell`.

## Examples

```bash
# Bash
daedalus completion bash >> ~/.bashrc

# Zsh
daedalus completion zsh >> ~/.zshrc

# Fish
daedalus completion fish > ~/.config/fish/completions/daedalus.fish
```

## Exit codes

| Code | Meaning |
|---|---|
| `0` | Completions printed successfully |
| `1` | Invalid shell |
