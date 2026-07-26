"""xbin completions: shell completion scripts for bash, zsh, and fish.

Usage:
    xbin completion bash    # output bash completion script
    xbin completion zsh     # output zsh completion script
    xbin completion fish    # output fish completion script

Install:
    xbin completion bash >> ~/.bashrc
    xbin completion zsh >> ~/.zshrc
    xbin completion fish > ~/.config/fish/completions/xbin.fish
"""

from __future__ import annotations

import sys


def _bash_completion() -> str:
    return """\
_xbin_completions()
{
    local cur prev
    COMPREPLY=()
    cur="${COMP_WORDS[COMP_CWORD]}"
    prev="${COMP_WORDS[COMP_CWORD-1]}"

    case ${prev} in
        build)
            COMPREPLY=($(compgen -f -- "${cur}"))
            return 0
            ;;
        --env-file|--key)
            COMPREPLY=($(compgen -f -- "${cur}"))
            return 0
            ;;
        --target)
            COMPREPLY=($(compgen -W "aarch64 x86_64" -- "${cur}"))
            return 0
            ;;
        --isolation)
            COMPREPLY=($(compgen -W "0 1 2" -- "${cur}"))
            return 0
            ;;
        --otel-protocol)
            COMPREPLY=($(compgen -W "grpc http/protobuf" -- "${cur}"))
            return 0
            ;;
    esac

    if [[ ${cur} == -* ]]; then
        COMPREPLY=($(compgen -W "\
--help --version --no-color \
build inspect keygen sign verify trust scan doctor clean env run upgrade completion \
" -- "${cur}"))
        return 0
    fi

    if [[ ${COMP_CWORD} -eq 1 ]]; then
        COMPREPLY=($(compgen -W "build inspect keygen sign verify trust scan doctor clean env run upgrade completion" -- "${cur}"))
        return 0
    fi

    local subcmd="${COMP_WORDS[1]}"
    case ${subcmd} in
        build)
            COMPREPLY=($(compgen -W "\
-o --output --key --isolation --seccomp --encrypt --squashfs --target \
-v --verbose -q --quiet --redetect --update --no-install --env-file \
--version-info --author --description --license --persist --include \
--tree-shake --minify --health-port --otel-endpoint --otel-protocol --cron \
" -- "${cur}"))
            ;;
        inspect)
            COMPREPLY=($(compgen -W "--json" -- "${cur}"))
            ;;
        doctor)
            COMPREPLY=($(compgen -W "-q --quiet --json --fix --force -f --strict" -- "${cur}"))
            ;;
        sign)
            COMPREPLY=($(compgen -W "--key" -- "${cur}"))
            ;;
        verify)
            COMPREPLY=($(compgen -W "--trusted-dir --json" -- "${cur}"))
            ;;
        keygen)
            COMPREPLY=($(compgen -W "--key-dir" -- "${cur}"))
            ;;
        scan)
            COMPREPLY=($(compgen -W "--json" -- "${cur}"))
            ;;
        clean)
            COMPREPLY=($(compgen -W "-f --force --all" -- "${cur}"))
            ;;
        run)
            COMPREPLY=($(compgen -f -- "${cur}"))
            ;;
    esac

    return 0
}
complete -F _xbin_completions xbin
"""


def _zsh_completion() -> str:
    return """\
#compdef xbin

_xbin() {
    local -a commands
    commands=(
        'build:Build a .xbin binary from an app directory'
        'inspect:Inspect a .xbin file'
        'keygen:Generate an Ed25519 signing keypair'
        'sign:Sign a .xbin file'
        'verify:Verify a .xbin file signature'
        'trust:Trust a public key'
        'scan:Scan directories for .xbin files'
        'doctor:Check system prerequisites'
        'clean:Clean xbin cache'
        'env:Show xbin environment info'
        'run:Run a .xbin file'
        'upgrade:Upgrade to latest release'
        'completion:Generate shell completion scripts'
    )

    _arguments -C \
        '1:command:->command' \
        '*::arg:->args'

    case $state in
        command)
            _describe 'command' commands
            ;;
        args)
            case $words[1] in
                build)
                    _arguments \
                        '(-o --output)'{-o,--output}'[Output path]:output:_files' \
                        '--key[Signing key path]:key:_files' \
                        '--isolation[Isolation mode]:isolation:(0 1 2)' \
                        '--seccomp[Enable seccomp BPF]' \
                        '--encrypt[Enable encryption]' \
                        '--squashfs[Use SquashFS]' \
                        '--target[Target arch]:target:(aarch64 x86_64)' \
                        '(-v --verbose)'{-v,--verbose}'[Verbose output]' \
                        '(-q --quiet)'{-q,--quiet}'[Quiet output]' \
                        '--redetect[Force re-detection]' \
                        '--update[Incremental rebuild]' \
                        '--no-install[Skip dependency install]' \
                        '--env-file[Environment file]:file:_files' \
                        '--version-info[Version string]:version' \
                        '--author[Author name]:author' \
                        '--description[App description]:description' \
                        '--license[License identifier]:license' \
                        '--persist[Enable persistent storage]' \
                        '--include[Include file/dir]:path:_files' \
                        '--tree-shake[Tree-shake node_modules]' \
                        '--minify[Minify JS/TS/CSS]' \
                        '--health-port[Health check port]:port' \
                        '--otel-endpoint[OTLP endpoint]:url' \
                        '--otel-protocol[OTLP protocol]:protocol:(grpc http/protobuf)' \
                        '*--cron[Cron task]:name:schedule' \
                        '*:app directory:_files -/'
                    ;;
                inspect|verify)
                    _arguments \
                        '--json[JSON output]' \
                        '*:file:_files'
                    ;;
                doctor)
                    _arguments \
                        '(-q --quiet)'{-q,--quiet}'[Quiet output]' \
                        '--json[JSON output]' \
                        '--fix[Auto-fix missing tools]' \
                        '(-f --force)'{-f,--force}'[Skip confirmation]' \
                        '--strict[Exit on missing tools]'
                    ;;
                sign)
                    _arguments \
                        '--key[Signing key]:key:_files' \
                        '*:file:_files'
                    ;;
                keygen)
                    _arguments \
                        '--key-dir[Key directory]:directory:_files -/'
                    ;;
                clean)
                    _arguments \
                        '(-f --force)'{-f,--force}'[Skip confirmation]' \
                        '--all[Remove everything]'
                    ;;
                scan)
                    _arguments \
                        '--json[JSON output]' \
                        '*:paths:_files -/'
                    ;;
                trust)
                    _arguments \
                        '*:public key:_files'
                    ;;
                run)
                    _arguments \
                        '*:file:_files' \
                        '*::args:'
                    ;;
            esac
            ;;
    esac
}

_xbin "$@"
"""


def _fish_completion() -> str:
    return """\
# Fish completion for xbin

# Subcommands
complete -c xbin -n "__fish_use_subcommand" -a build -d "Build a .xbin binary"
complete -c xbin -n "__fish_use_subcommand" -a inspect -d "Inspect a .xbin file"
complete -c xbin -n "__fish_use_subcommand" -a keygen -d "Generate signing keys"
complete -c xbin -n "__fish_use_subcommand" -a sign -d "Sign a .xbin file"
complete -c xbin -n "__fish_use_subcommand" -a verify -d "Verify signature"
complete -c xbin -n "__fish_use_subcommand" -a trust -d "Trust a public key"
complete -c xbin -n "__fish_use_subcommand" -a scan -d "Scan for .xbin files"
complete -c xbin -n "__fish_use_subcommand" -a doctor -d "Check prerequisites"
complete -c xbin -n "__fish_use_subcommand" -a clean -d "Clean cache"
complete -c xbin -n "__fish_use_subcommand" -a env -d "Show environment"
complete -c xbin -n "__fish_use_subcommand" -a run -d "Run a .xbin"
complete -c xbin -n "__fish_use_subcommand" -a upgrade -d "Upgrade xbin"
complete -c xbin -n "__fish_use_subcommand" -a completion -d "Shell completions"

# Global flags
complete -c xbin -l help -s h -d "Show help"
complete -c xbin -l version -d "Show version"
complete -c xbin -l no-color -d "Disable colored output"

# build flags
complete -c xbin -n "__fish_seen_subcommand_from build" -s o -l output -r -d "Output path"
complete -c xbin -n "__fish_seen_subcommand_from build" -l key -r -F -d "Signing key"
complete -c xbin -n "__fish_seen_subcommand_from build" -l isolation -r -d "Isolation mode"
complete -c xbin -n "__fish_seen_subcommand_from build" -l seccomp -d "Enable seccomp"
complete -c xbin -n "__fish_seen_subcommand_from build" -l encrypt -d "Enable encryption"
complete -c xbin -n "__fish_seen_subcommand_from build" -l squashfs -d "Use SquashFS"
complete -c xbin -n "__fish_seen_subcommand_from build" -l target -r -xa "aarch64 x86_64" -d "Target arch"
complete -c xbin -n "__fish_seen_subcommand_from build" -s v -l verbose -d "Verbose"
complete -c xbin -n "__fish_seen_subcommand_from build" -s q -l quiet -d "Quiet"
complete -c xbin -n "__fish_seen_subcommand_from build" -l redetect -d "Force re-detection"
complete -c xbin -n "__fish_seen_subcommand_from build" -l update -d "Incremental rebuild"
complete -c xbin -n "__fish_seen_subcommand_from build" -l no-install -d "Skip dep install"
complete -c xbin -n "__fish_seen_subcommand_from build" -l env-file -r -F -d "Env file"
complete -c xbin -n "__fish_seen_subcommand_from build" -l version-info -r -d "Version"
complete -c xbin -n "__fish_seen_subcommand_from build" -l author -r -d "Author"
complete -c xbin -n "__fish_seen_subcommand_from build" -l description -r -d "Description"
complete -c xbin -n "__fish_seen_subcommand_from build" -l license -r -d "License"
complete -c xbin -n "__fish_seen_subcommand_from build" -l persist -d "Enable persistent storage"
complete -c xbin -n "__fish_seen_subcommand_from build" -l include -r -F -d "Include file/dir"
complete -c xbin -n "__fish_seen_subcommand_from build" -l tree-shake -d "Tree-shake"
complete -c xbin -n "__fish_seen_subcommand_from build" -l minify -d "Minify"
complete -c xbin -n "__fish_seen_subcommand_from build" -l health-port -r -d "Health port"
complete -c xbin -n "__fish_seen_subcommand_from build" -l otel-endpoint -r -d "OTLP endpoint"
complete -c xbin -n "__fish_seen_subcommand_from build" -l otel-protocol -r -xa "grpc http/protobuf"
complete -c xbin -n "__fish_seen_subcommand_from build" -l cron -r -d "Cron task"

# inspect/verify flags
complete -c xbin -n "__fish_seen_subcommand_from inspect verify" -l json -d "JSON output"

# doctor flags
complete -c xbin -n "__fish_seen_subcommand_from doctor" -s q -l quiet -d "Quiet"
complete -c xbin -n "__fish_seen_subcommand_from doctor" -l json -d "JSON output"
complete -c xbin -n "__fish_seen_subcommand_from doctor" -l fix -d "Auto-fix"
complete -c xbin -n "__fish_seen_subcommand_from doctor" -s f -l force -d "Skip confirmation"
complete -c xbin -n "__fish_seen_subcommand_from doctor" -l strict -d "Exit on failure"

# sign flags
complete -c xbin -n "__fish_seen_subcommand_from sign" -l key -r -F -d "Signing key"

# keygen flags
complete -c xbin -n "__fish_seen_subcommand_from keygen" -l key-dir -r -d "Key directory"

# clean flags
complete -c xbin -n "__fish_seen_subcommand_from clean" -s f -l force -d "Skip confirmation"
complete -c xbin -n "__fish_seen_subcommand_from clean" -l all -d "Remove everything"

# scan flags
complete -c xbin -n "__fish_seen_subcommand_from scan" -l json -d "JSON output"
"""


_COMPLETIONS = {
    "bash": _bash_completion,
    "zsh": _zsh_completion,
    "fish": _fish_completion,
}


def completion(shell: str) -> int:
    """Print shell completion script and return exit code."""
    if shell not in _COMPLETIONS:
        print(
            f"[xbin] error: unsupported shell '{shell}' — choose bash, zsh, or fish",
            file=sys.stderr,
        )
        return 1
    print(_COMPLETIONS[shell]())
    return 0
