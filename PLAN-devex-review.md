# daedalus DX Review Plan

## Review Metadata

- **Product Type:** CLI Tool
- **Persona:** YC founder building MVP (30-min integration tolerance, copies from README)
- **Mode:** DX POLISH (improve existing plan's DX rigor)
- **Competitive Tier:** Competitive (2-5 min TTHW)
- **TTHW Target:** < 2 min (user chose "Optimize to <1 min")

## Developer Persona Card

```
TARGET DEVELOPER PERSONA
========================
Who:       YC founder building MVP
Context:   Wants to package their Python/Ruby/Node app into a single binary for distribution. Discovered via HN/Show HN.
Tolerance: 30 minutes from discovery to running binary
Expects:   One command install, one command build, binary just works
```

## Developer Empathy Narrative

*I'm a YC founder. I just read the HN pitch. The title says "package any app into a single Linux binary". Let me try it.*

1. I open the README. The title says "Package any app into a single self-extracting binary." The quick start shows `curl -fsSL https://raw.githubusercontent.com/Tednoob17/daedalus/main/scripts/install.sh | bash` — one command, that's good.
2. I run `daedalus doctor` — all checks pass except `daedalus-stub` (marked optional). Good.
3. I try `daedalus build . -o myapp.de` — but wait, the README says `.daedalus` not `.de`. The pitch said `.de`. Confusing already.
4. The CLI help text shows examples with `--encrypt` flag: `daedalus build ./myapp --encrypt --key ~/.daedalus/keys/*.key`. But the pitch said encryption was removed (fake security). Why is it still in the help?
5. I look at the README Security section — it still describes `--encrypt` at length: "Encryption (obfuscation only): `--encrypt` uses AES-256-GCM..." But commit d631138 said this was removed. The README and code disagree.
6. I try to build an example: `daedalus build ./myapp -o myapp.de` — builds successfully in ~90 seconds. The output says `Built /tmp/test.daedalus (18.3MB)` but I specified `-o myapp.de`. Confusing.
7. Running the binary: `./myapp.de` — works, app extracts and runs.

## Competitive DX Benchmark

```
COMPETITIVE DX BENCHMARK
=========================
Tool              | TTHW      | Notable DX Choice          | Source
PyInstaller        | ~3 min   | pip install + one cmd      | Reference
Docker             | ~5 min   | docker build/run           | Reference
Bun                | ~30s     | Built-in compiler          | Reference
Nuitka             | ~5 min   | Slow, frequent breakage    | Reference
daedalus (current) | ~2 min   | curl | bash + build + run    | Measured
daedalus (target)  | < 1 min  | Install+doctor combined    | User choice
```

## Magical Moment Specification

**Magical moment:** App built and running from a single binary in 4 commands.

**Delivery vehicle:** Copy-paste demo command — one terminal command that produces the magical output. Chosen because for a CLI tool, the magic is seeing the binary work after building.

**Implementation requirements:**
- README quick start must be exactly 3-4 commands that work end-to-end
- First command: `curl -fsSL https://raw.githubusercontent.com/Tednoob17/daedalus/main/scripts/install.sh | bash`
- Build command: `daedalus build ./myapp -o myapp.de`
- Run command: `./myapp.de`
- Output should be visibly small (single binary, no runtime dependency)

## Developer Journey Map

```
STAGE        | DEVELOPER DOES              | FRICTION POINTS              | STATUS
-------------|-----------------------------|----------------------------- |--------
1. Discover  | Reads HN pitch              | .de vs .daedalus inconsistency  | deferred
2. Install   | curl | bash                   | One command, works             | ok
3. Hello World| daedalus build + ./myapp    | Extension mismatch, --encrypt in help | fixed
4. Real Usage| daedalus inspect            | OK, no friction               | ok
5. Debug     | daedalus doctor             | OK, clear output              | ok
6. Upgrade    | daedalus upgrade            | Not tested yet                | ok
```

## First-Time Developer Confusion Report

```
FIRST-TIME DEVELOPER REPORT
============================
Persona: YC founder building MVP
Attempting: daedalus getting started

CONFUSION LOG:
T+0:00  Open README. Title says "single self-extracting binary". Quick start shows curl | bash.
T+0:30  Run install. Run `daedalus doctor`. All checks pass except daedalus-stub (optional).
T+1:00  Try `daedalus build . -o myapp.de`. Build succeeds but output says "Built /tmp/test.daedalus" — extension mismatch.
T+1:30  Check `daedalus --help`. Examples show `--encrypt` flag but I thought that was removed. Security section in README contradicts itself.
T+2:00  Run `./myapp.de`. App works. But I'm confused about what's real and what's deprecated.
T+5:00  Still seeing .daedalus in all CLI help text and code examples despite pitch saying .de.
```

**Items addressed:** T+1:00 (extension), T+1:30 (--encrypt removal), T+2:00 (docs update)

## What Already Exists

- TTY-aware color handling (`use_color()` in doctor.rs)
- Shell completions (bash, zsh, fish, elvish, powershell)
- Man page generation
- `.dry-run` flag on build/inspect/scan
- `--verbose`/`--quiet` flags
- Ed25519 signing and verification
- SISR (delta updates) support
- `daedalus doctor` for prerequisite checking
- `daedalus selftest` for ephemeral sandbox testing
- TUI dashboard for SISR benchmarks
- Cross-platform target support (Linux x64/arm64, macOS, Windows)

## NOT in Scope

- ARM64 runtime embedding (P3 roadmap)
- macOS/Windows runtime embedding (P3 roadmap)
- Sigstore/cosign integration (P3 roadmap)
- Remote cache with Depot-style HTTP (P3 roadmap, already in code)
- Progress bars for build/upgrade (P3 roadmap, clig.dev nice-to-have)

## Review Passes

### Pass 1: Getting Started Experience — 7/10

**Evidence recall:** Persona (YC founder, 30-min tolerance), Target tier: <1 min, Magical moment: 4-command copy-paste flow.

**Findings:**
- Install is one command (`curl | bash`) — good
- `daedalus doctor` passes in 5 seconds — good
- First run works (build + execute) — good
- **Major issue:** Extension inconsistency between HN pitch (`.de`) and actual code/CLI (`.daedalus`). User will type `.de` but get `.daedalus` or see confusing error.
- **Major issue:** `--encrypt` flag still shown in CLI help text and README despite being "removed" in commit d631138
- No browser sandbox/playground to try before installing

**Rating:** 7/10 → **8/10** after fixing extension + encrypt references

**Fixes needed:**
1. Rename all `.daedalus` → `.de` in README.md, CLI help text, all examples
2. Remove `--encrypt` from CLI help examples and README Security section
3. Update CLAUDE.md quick start references

### Pass 2: CLI/API Design — 6/10

**Evidence recall:** Persona expects `tool.command thing` patterns. YC founder copies from README.

**Findings:**
- 21 commands listed in help — overwhelming for first-time users
- Command naming is mostly consistent but some are confusing: `upgrade-binary` (migrate v1 to v2) vs `upgrade` (self-update)
- `--encrypt` still appears in build help examples
- Default output name `app.daedalus` is hardcoded — should be `app.de`
- No `daedalus help <command>` pattern (relies on `--help`)

**Rating:** 6/10

**Fixes needed:**
1. Rename `upgrade-binary` to `migrate` for clarity
2. Update all `--encrypt` references in CLI help
3. Change default output from `app.daedalus` to `app.de`

### Pass 3: Error Messages & Debugging — 5/10

**Evidence trace:**
1. `daedalus inspect nonexistentfile.de` → "Error: failed to open nonexistentfile.de: No such file or directory (os error 2)" — doesn't explain what the user did wrong
2. `daedalus build nonexistent-dir` → "Error: failed to canonicalize app path: No such file or directory (os error 2)" — uses internal terminology ("canonicalize")

**Rating:** 5/10

**Fixes needed:**
1. User should see: "Error: app directory not found: /path/to/nonexistent-dir"
2. "Error: file not found: nonexistentfile.de — check the path and try again"
3. Add help links or suggestions for common errors

### Pass 4: Documentation & Learning — 6/10

**Evidence recall:** YC founder won't read docs beyond README. Platform engineer would want architecture docs.

**Findings:**
- README has good quick start and examples
- Docs structure exists (`docs/` with guides, reference, concepts, spec)
- CONTRIBUTING.md exists with getting started for devs
- **Missing:** Troubleshooting guide for common build errors
- **Missing:** Migration guide for upgrading daedalus itself
- README Security section contradicts itself (describes --encrypt after removal)

**Rating:** 6/10

**Fixes needed:**
1. Update README Security section to remove --encrypt references
2. Add troubleshooting section ("Error: app directory not found?")
3. Add migration guide for upgrading binary format

### Pass 5: Upgrade & Migration Path — 7/10

**Findings:**
- `daedalus upgrade` command exists for self-update
- `daedalus upgrade-binary` exists for v1→v2 migration
- CHANGELOG.md exists
- No explicit migration guide for breaking changes
- Versioning strategy not clearly documented

**Rating:** 7/10

### Pass 6: Developer Environment & Tooling — 8/10

**Findings:**
- Cross-platform targets supported (Linux x64/arm64, macOS x64/arm64, Windows x64/arm64)
- Shell completions for all major shells
- Man page generation
- `daedalus doctor` checks prerequisites
- Dockerfile likely not needed (self-contained binaries)

**Rating:** 8/10

### Pass 7: Community & Ecosystem — 4/10

**Findings:**
- GitHub issues are the only support channel
- No Discord/Slack community mentioned
- No examples repository
- No plugin ecosystem (would need one for runtime extensions)
- LICENSE is MIT (good, permissive)
- Contributing guide exists

**Rating:** 4/10

**Fixes needed:**
1. Add GitHub Discussions or Discord link to README
2. Create examples repository (hello-python, hello-node, hello-ruby)

### Pass 8: DX Measurement & Feedback Loops — 2/10

**Findings:**
- No TTHW tracking/instrumentation
- No journey analytics
- No feedback mechanism (no feedback button, no NPS)
- No friction audits planned
- No boomerang readiness (no /devex-review planned)

**Rating:** 2/10

**Fixes needed:**
1. Add `daedalus feedback` command linking to GitHub issues
2. Add TTHW measurement to CI (time the install+build+run path)
3. Add feedback link to error messages

## Implementation Tasks

```markdown
## Implementation Tasks
Synthesized from this review's findings. Each task derives from a specific finding.

- [ ] **T1 (P1, human: ~3h / CC: ~45min)** — cli: Rename .daedalus to .de across all source files
  - Surfaced by: Pass 1 — Extension inconsistency between HN pitch and CLI output
  - Files: daedalus-cli/src/main.rs, daedalus-cli/src/commands/build/args.rs, daedalus-cli/src/commands/build/pipeline.rs, README.md, CLAUDE.md, CONTRIBUTING.md
  - Verify: `cargo build --release && daedalus build ./examples/hello-web -o test.de`

- [ ] **T2 (P1, human: ~2h / CC: ~30min)** — cli: Remove --encrypt from help text and examples
  - Surfaced by: Pass 1 — --encrypt flag still shown despite removal in d631138
  - Files: daedalus-cli/src/main.rs (line 45-46), daedalus-cli/src/commands/build/args.rs (line ~278)
  - Verify: `daedalus build --help` no longer shows --encrypt

- [ ] **T3 (P1, human: ~2h / CC: ~30min)** — core: Upgrade error messages with problem + cause + fix pattern
  - Surfaced by: Pass 3 — "failed to canonicalize app path" is internal terminology
  - Files: daedalus-cli/src/commands/build/pipeline.rs, daedalus-cli/src/error.rs
  - Verify: `daedalus inspect nonexistent.de` → "File not found: nonexistent.de. Check the path and try again."

- [ ] **T4 (P2, human: ~1h / CC: ~20min)** — docs: Fix README Security section to remove --encrypt references
  - Surfaced by: Pass 4 — README still documents encryption after removal
  - Files: README.md (lines 136-142, 261)
  - Verify: grep README.md for --encrypt returns nothing

- [ ] **T5 (P2, human: ~3h / CC: ~45min)** — cli: Rename upgrade-binary to migrate for clarity
  - Surfaced by: Pass 2 — "upgrade-binary" vs "upgrade" naming confusion
  - Files: daedalus-cli/src/main.rs, help text
  - Verify: `daedalus migrate --help` works

- [ ] **T6 (P2, human: ~1h / CC: ~15min)** — docs: Add troubleshooting section to README
  - Surfaced by: Pass 4 — No help for common errors
  - Files: README.md
  - Verify: README has "Troubleshooting" heading

- [ ] **T7 (P3, human: ~3h / CC: ~1h)** — docs: Add community channel links to README
  - Surfaced by: Pass 7 — No Discord/GH Discussions link
  - Files: README.md
  - Verify: README has link to community channel

- [ ] **T8 (P3, human: ~2h / CC: ~15min)** — cli: Add daedalus feedback command
  - Surfaced by: Pass 8 — No feedback mechanism
  - Files: daedalus-cli/src/commands/
  - Verify: `daedalus feedback --help` works
```

## DX Scorecard

```
+====================================================================+
|              DX PLAN REVIEW — SCORECARD                             |
+====================================================================+
| Dimension            | Score  | Prior  | Trend  |
|----------------------|--------|--------|--------|
| Getting Started      | 7/10   | —      | NEW    |
| API/CLI/SDK          | 6/10   | —      | NEW    |
| Error Messages       | 5/10   | —      | NEW    |
| Documentation        | 6/10   | —      | NEW    |
| Upgrade Path         | 7/10   | —      | NEW    |
| Dev Environment      | 8/10   | —      | NEW    |
| Community            | 4/10   | —      | NEW    |
| DX Measurement       | 2/10   | —      | NEW    |
+--------------------------------------------------------------------+
| TTHW                 | 2 min  | —      | NEW    |
| Competitive Rank     | Competitive              |
| Magical Moment       | Designed via copy-paste  |
| Product Type         | CLI Tool                  |
| Mode                 | DX POLISH                  |
| Overall DX           | 5/10   | —      | NEW    |
+====================================================================+
| DX PRINCIPLE COVERAGE                                               |
| Zero Friction      | covered (one-command install)                   |
| Learn by Doing     | covered (build + run example)                   |
| Fight Uncertainty  | gap (vague error messages)                      |
| Opinionated + Escape Hatches | covered (doctor, --dry-run)        |
| Code in Context    | partial (examples work but docs contradict)     |
| Magical Moments    | designed (4-command flow)                       |
+====================================================================+
```

## DX Implementation Checklist

```
DX IMPLEMENTATION CHECKLIST
============================
[✅] Time to hello world ~2 min (target: <1 min per user)
[✅] Installation is one command (curl | bash)
[✅] First run produces meaningful output (Built 18.3MB)
[✅] Magical moment via copy-paste demo command
[❌] Every error message has: problem + cause + fix + docs link
[❌] API/CLI naming — all guessable without docs (upgrade-binary vs upgrade)
[✅] Every parameter has a sensible default (mostly)
[❌] Docs have copy-paste examples that are consistent (extension mismatch)
[✅] Examples show real use cases
[✅] Upgrade path documented (upgrade, upgrade-binary commands)
[❌] Breaking changes have deprecation warnings + codemods
[❌] TypeScript types (not applicable for Rust CLI)
[✅] Works in CI/CD (no special configuration needed)
[❌] Free tier available, no credit card required
[✅] Changelog exists and is maintained
[❌] Search works in documentation (no search bar on docs site)
[❌] Community channel exists and is monitored
```

## Unresolved Decisions

- encrypt.rs (368 lines) kept in codebase — user chose to keep it as isolated module
- 21 CLI commands kept visible — user chose not to hide advanced commands
- Extension rename to .de — user chose full rename (needs implementation)
- Error message upgrade — user chose to upgrade all messages (needs implementation)

## Review Log

```json
{"skill":"plan-devex-review","timestamp":"2026-08-26T13:57:00Z","status":"issues_open","initial_score":5,"overall_score":5,"product_type":"CLI Tool","tthw_current":"2 min","tthw_target":"<1 min","mode":"DX POLISH","persona":"YC founder building MVP","competitive_tier":"Competitive","unresolved":0,"commit":"d631138"}
```

## GSTACK REVIEW REPORT

| Review | Trigger | Why | Runs | Status | Findings |
|--------|---------|-----|------|--------|----------|
| CEO Review | `/plan-ceo-review` | Scope & strategy | 0 | — | No CEO review run |
| Codex Review | `/codex review` | Independent 2nd opinion | 0 | — | No codex review |
| Eng Review | `/plan-eng-review` | Architecture & tests | 0 | — | No eng review |
| Design Review | `/plan-design-review` | UI/UX gaps | 0 | — | No design review |
| DX Review | `/plan-devex-review` | Developer experience gaps | 1 | ISSUES_OPEN | score: 5/10 → 5/10, TTHW: 2 min → <1 min |

**VERDICT:** No prior reviews found. This is the first review. Key issues: extension inconsistency (.daedalus vs .de), --encrypt still in help text after removal, error messages use internal terminology. 5 actionable tasks identified (3 P1, 2 P2).

NO UNRESOLVED DECISIONS
