# Eval results

Each run of `python3 evals/run.py --record` adds a section: date, model, jobs passed, checks
passed and cost, then one row per job. See README.md for what the jobs and checks are.

### 2026-10-08 · sonnet · 1/2 jobs passed · 10/11 checks · $0.53

| Job | Skill | Result | Checks | Tool calls | Time | Notes |
| --- | --- | --- | --- | --- | --- | --- |
| drums-house | drum-programming | FAIL | 5/6 | 7 | 18.4s | looked or measured before finishing (finish routine) (0 looks/measures) |
| master-streaming | mastering | pass | 5/5 | 18 | 306.2s | all checks passed |

### 2026-10-08 · sonnet · 1/1 jobs passed · 6/6 checks · $0.12

After `ryolune-mcp` started carrying its notes (song state, finish-routine reminder) in
`structuredContent` too: Claude Code shows that instead of the text, so the first run's agent
never saw the reminder.

| Job | Skill | Result | Checks | Tool calls | Time | Notes |
| --- | --- | --- | --- | --- | --- | --- |
| drums-house | drum-programming | pass | 6/6 | 6 | 26.8s | all checks passed |
