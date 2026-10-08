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

### 2026-10-08 · opus · 12/13 jobs passed · 65/68 checks · $4.71

The full release run (0.16.0). export-stems failed because no export could write into a folder
that did not exist yet; fixed (exports make their folders) and re-run below.

| Job | Skill | Result | Checks | Tool calls | Time | Notes |
| --- | --- | --- | --- | --- | --- | --- |
| drums-house | drum-programming | pass | 6/6 | 6 | 49.1s | all checks passed |
| bass-chords | bass-and-chords | pass | 8/8 | 7 | 136.4s | all checks passed |
| compose-lofi | compose-from-brief | pass | 8/8 | 18 | 913.8s | all checks passed |
| arrange-demo | arrangement | pass | 6/6 | 12 | 375.9s | all checks passed |
| fix-clipping | mixing | pass | 5/5 | 10 | 838.9s | all checks passed |
| master-streaming | mastering | pass | 5/5 | 26 | 706.6s | all checks passed |
| master-broadcast | mastering | pass | 5/5 | 9 | 365.0s | all checks passed |
| darker-pad | sound-design | pass | 4/4 | 7 | 98.2s | all checks passed |
| melody | melody-and-hooks | pass | 5/5 | 8 | 227.6s | all checks passed |
| score-cut | score-to-picture | pass | 5/5 | 15 | 504.2s | all checks passed |
| export-stems | stems-and-export | FAIL | 1/4 | 4 | 42.1s | a 44.1 kHz 16-bit WAV mix (no mix.wav); one stem per track (0 files); looked or measured before finishing (finish routine) (0 looks/measures) |
| ritardando | arrangement | pass | 4/4 | 5 | 65.8s | all checks passed |
| fade-out | automation-and-movement | pass | 3/3 | 3 | 92.7s | all checks passed |

### 2026-10-08 · opus · 0/1 jobs passed · 3/4 checks · $0.08

export-stems after the folder fix: the files are right; the agent read the exports' own peak and
clipping report instead of `harness.measure`, which the job's check did not count yet.

| Job | Skill | Result | Checks | Tool calls | Time | Notes |
| --- | --- | --- | --- | --- | --- | --- |
| export-stems | stems-and-export | FAIL | 3/4 | 2 | 153.2s | looked or measured before finishing (finish routine) (0 looks/measures) |

### 2026-10-08 · opus · 1/1 jobs passed · 4/4 checks · $0.08

export-stems with its finish check counting the exports' own report (peak, clipping): the agent
reported the mix clips at +1.7 dBFS and offered fixes without changing the song.
Release result for 0.16.0 with Opus: 13/13 jobs.

| Job | Skill | Result | Checks | Tool calls | Time | Notes |
| --- | --- | --- | --- | --- | --- | --- |
| export-stems | stems-and-export | pass | 4/4 | 2 | 182.5s | all checks passed |
