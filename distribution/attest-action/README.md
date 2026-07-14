# attest-action

Run [attest](https://github.com/harrisonwang/attest) as a composite GitHub Action and annotate stale repository documentation directly in pull requests.

```yaml
name: Documentation drift

on:
  pull_request:
  push:

permissions:
  contents: read

jobs:
  attest:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v6
      - uses: harrisonwang/attest-action@v0.1.0
```

## Inputs

| Input | Default | Purpose |
|---|---|---|
| `version` | `latest` | Published `@harrisonwang/attest` version to execute |
| `args` | `check --format github` | Arguments passed to the `attest` CLI |

Existing debt can be accepted in the target repository with `attest baseline update`; the action then fails only for unbaselined `broken` findings. It never executes commands extracted from repository documentation.

The repository smoke workflow runs the published `0.1.0` package against `fixtures/clean/AGENTS.md`, which contains a real path binding to `fixtures/clean/README.md`.
