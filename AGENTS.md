# Commit and push

The user requires agents to commit and push completed, verified changes by default. Do not finish authorized repository work with changes left only in the local checkout. Use a short commit message, push the task branch to the configured remote, verify the remote contains the commit, and report its ID. An explicit user instruction to keep work local takes precedence.

## Concurrent agents

- Preserve other agents' files and changes. Stage only your own files or hunks; never use a blanket `git add -A`.
- Serialize staging, commits, and pushes by holding exclusive `FileShare.None` access to `.artifacts/git-publish.lock` for the entire Git operation. If it is held, continue independent work and retry later.
- Before staging, verify the shared index is empty. If another agent's changes are staged, leave them intact and wait for that agent to finish.
- Do not reset or stash another agent's work, switch the shared branch, or force-push.
- If a commit or push fails, preserve the work and report the specific failure instead of claiming publication succeeded.
