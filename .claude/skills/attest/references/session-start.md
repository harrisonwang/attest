# SessionStart hook recipe

Install `attest` on the machine, then use the shareable project configuration already included at `.claude/settings.json`:

```json
{
  "hooks": {
    "SessionStart": [
      {
        "matcher": "startup|resume",
        "hooks": [
          {
            "type": "command",
            "command": "bash \"${CLAUDE_PROJECT_DIR}/.claude/skills/attest/scripts/session-start.sh\"",
            "timeout": 30
          }
        ]
      }
    ]
  }
}
```

The script never executes commands extracted from documentation. It exits quietly when `attest` is unavailable, never blocks startup on findings, and limits injected output to 80 lines. Set `ATTEST_BINARY` in the hook environment to use a non-default binary path.
