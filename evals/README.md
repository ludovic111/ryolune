# ryolune agent evals

lsuite's HARNESS.md part 7: scripted music jobs, run headless with a real model, scored by
automatic checks on the song the agent leaves. Run them before each release; a harness change
(brief, skills, commands) that lowers the pass rate does not ship.

## How a job runs

1. A **fixture** song is built with `ryolune-cli` (`fixtures.py`): an empty song, the Nightfall
   demo, chords waiting for a melody, a clipping mix, a quiet mix, a bright pad, a kimchi cut in
   the hand-off inbox.
2. **Claude Code** gets the job's request, with ryolune's MCP server hosting that song
   headless (`ryolune-mcp --file song.ryolune`) and nothing else (`--tools ""`,
   `--strict-mcp-config`). The server's instructions are the harness brief; the skills are its
   prompts and resources and `harness_skill`. Claude Code uses its own sign-in (no key needed on
   a developer's machine), or `ANTHROPIC_API_KEY` when it is set.
3. **Checks** (`jobs.py`) read the resulting song with `ryolune-cli` (`session.get`,
   `harness.measure`): tempo and key, tracks and notes, notes in key, registers, sections,
   loudness and true peak against the target, clipping, files written, the person's tracks and
   notes kept, and that the agent looked or measured before it finished (the finish routine).

## Running

```sh
cargo build -p ryolune-tools            # or --release; the runner finds target/{release,debug}
python3 evals/run.py --list             # the jobs
python3 evals/run.py                    # all of them (about 13 × a few minutes)
python3 evals/run.py --jobs drums-house,master-streaming --record
```

Options: `--model` (default `sonnet`, or `RYOLUNE_EVAL_MODEL`), `--budget` (USD per job, Claude
Code's `--max-budget-usd`, default 3), `--timeout` (seconds per job, default 900), `--keep` (keep
each job's folder: the song, `before.ryolune`, the MCP config and `transcript.jsonl`), `--record`
(append the run to `RESULTS.md`). Every run writes `results/<time>.json` (not committed).

Every job runs in its own scratch folder: settings, data, the lsuite home and the control file
all point there, so an eval never touches your songs, settings or a running ryolune.

## Adding a job

Add a fixture to `fixtures.py` if none fits, then an entry to `JOBS` in `jobs.py`: an id, the
fixture, the skill it exercises, the request in a person's words, and checks. A check is a
function `(song, ctx) -> (passed, detail)` decorated with `@check("what it proves")`; `ctx`
holds the song `before`, the agent's tool calls and the work folder.
