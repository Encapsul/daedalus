# Useful GitHub CLI Commands

All `gh` commands used for managing x.bin from the terminal.

---

## Auth & scopes

```bash
gh auth status                                          # check current auth + scopes
gh auth refresh -h github.com -s admin:ssh_signing_key  # add signing key scope
```

## User info

```bash
gh api user                                             # current user (login, email, id)
```

## SSH keys (authentication)

```bash
gh api user/keys                                        # list all auth keys
gh api user/keys/<id>                                   # get one key details
gh api user/keys/<id> -X DELETE                         # delete a key
```

## SSH keys (signing)

```bash
gh api user/ssh_signing_keys --paginate                 # list all signing keys
gh api user/ssh_signing_keys/<id>                       # get one signing key details
gh ssh-key add ~/.ssh/git_signing_key.pub --type signing --title "name"  # add signing key via CLI
```

## Releases

```bash
gh release list                                         # list all releases
gh release view <tag>                                   # show release details + assets
gh release create <tag> --title "Title" --notes "..."   # create release from CLI
gh release upload <tag> file.tar.gz                     # upload asset to existing release
```

## Workflow runs

```bash
gh run list                                             # list recent runs
gh run list --limit 5                                   # last 5 runs
gh run view <run-id>                                    # show run status + jobs
gh run view <run-id> --repo owner/repo                  # show run in specific repo
gh run view --job=<job-id>                              # show specific job details
gh run view --job=<job-id> --log-failed                 # show failed job logs
gh run watch <run-id>                                   # live-watch run progress
```

## Tags verification (check if GitHub recognizes SSH signature)

```bash
gh api repos/<owner>/<repo>/git/ref/tags/<tag>          # get tag SHA
gh api repos/<owner>/<repo>/git/tags/<sha>              # get tag object
# Check verification field in JSON:
# - verified: true → signature recognized
# - verified: false, reason: unknown_key → key not in signing keys
# - verified: false, reason: missing_public_key → key not uploaded
```

## Dependabot

```bash
gh api repos/<owner>/<repo>/dependabot/alerts            # list security alerts
gh api repos/<owner>/<repo>/dependabot/alerts/<id> -X PATCH -f state=dismissed  # dismiss
gh api repos/<owner>/<repo>/git/refs/heads/<branch> -X DELETE  # delete branch
gh api repos/<owner>/<repo>/pulls?head=owner:branch      # find PR for branch
gh pr list --repo owner/repo                            # list open PRs
gh pr view <number> --repo owner/repo                   # view PR details
```

## Branches & refs

```bash
git ls-remote --tags origin                             # list remote tags + SHAs
git push --force --tags origin                          # force-push all tags (USER ONLY)
```

## Workflow file checks

```bash
gh api repos/<owner>/<repo>/actions/workflows           # list all workflows
gh api repos/<owner>/<repo>/actions/workflows/<id>/runs # list runs for a workflow
```
