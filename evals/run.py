#!/usr/bin/env python3
"""Run ryolune's agent evals (lsuite's HARNESS.md part 7).

Each job starts from a fixture song, gives a real model one request through Claude Code with
ryolune's MCP server on that song (headless: `ryolune-mcp --file`), and scores the song it
leaves with automatic checks. Claude Code uses its own sign-in, so no key is needed on a
developer's machine; with ANTHROPIC_API_KEY set it uses that key instead.

    python3 evals/run.py                       # every job
    python3 evals/run.py --jobs drums-house,master-streaming --record
    python3 evals/run.py --list

Build the tools first (`cargo build -p ryolune-tools`, or --release); --bin-dir points
elsewhere. --record appends the run to evals/RESULTS.md.
"""
import argparse
import datetime
import json
import os
import shutil
import subprocess
import sys
import tempfile
import time
import traceback
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent
sys.path.insert(0, str(HERE))

from fixtures import FIXTURES  # noqa: E402
from jobs import JOBS  # noqa: E402
from song import Cli, Song  # noqa: E402


def find_bins(explicit):
    candidates = [Path(explicit)] if explicit else []
    target = Path(os.environ.get("CARGO_TARGET_DIR", ROOT / "target"))
    candidates += [target / "release", target / "debug"]
    for d in candidates:
        if (d / "ryolune-cli").exists() and (d / "ryolune-mcp").exists():
            return d
    sys.exit("ryolune-cli and ryolune-mcp not found: cargo build -p ryolune-tools (or pass --bin-dir)")


def find_claude(explicit):
    for c in [explicit, shutil.which("claude"), str(Path.home() / ".local/bin/claude")]:
        if c and Path(c).exists():
            return c
    sys.exit("The claude CLI was not found (install Claude Code or pass --claude)")


def run_agent(claude, model, prompt, mcp_config, work, env, timeout, budget):
    args = [claude, "-p", "--output-format", "stream-json", "--verbose", "--no-session-persistence",
            "--setting-sources", "", "--tools", "", "--mcp-config", str(mcp_config), "--strict-mcp-config",
            "--allowedTools", "mcp__ryolune__*", "--model", model]
    if budget:
        args += ["--max-budget-usd", str(budget)]
    started = time.time()
    proc = subprocess.Popen(args, cwd=work, env=env, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                            stderr=subprocess.PIPE, text=True)
    try:
        out, err = proc.communicate(prompt, timeout=timeout)
    except subprocess.TimeoutExpired:
        proc.kill()
        out, err = proc.communicate()
        err += f"\n(timed out after {timeout}s)"
    tools, result, text = [], {}, []
    for line in out.splitlines():
        try:
            event = json.loads(line)
        except json.JSONDecodeError:
            continue
        if event.get("type") == "assistant":
            for block in event.get("message", {}).get("content", []):
                if block.get("type") == "tool_use":
                    tools.append(block.get("name", ""))
                elif block.get("type") == "text":
                    text.append(block.get("text", ""))
        elif event.get("type") == "result":
            result = event
    (Path(work) / "transcript.jsonl").write_text(out)
    return {
        "tools": tools,
        "reply": (text[-1] if text else "")[-1500:],
        "seconds": round(time.time() - started, 1),
        "costUsd": result.get("total_cost_usd"),
        "turns": result.get("num_turns"),
        "error": None if proc.returncode == 0 else (err.strip()[-800:] or f"exit {proc.returncode}"),
    }


def run_job(job, args, bins, claude, root):
    work = root / job["id"]
    work.mkdir(parents=True)
    scratch = work / "scratch"
    env = dict(os.environ,
               RYOLUNE_SETTINGS=str(scratch / "settings.json"),
               RYOLUNE_DATA_DIR=str(scratch / "data"),
               RYOLUNE_CONTROL=str(scratch / "no-app-control.json"),
               LSUITE_HOME=str(scratch / "lsuite"),
               RYOLUNE_NO_UPDATE="1",
               # Renders take seconds (minutes in a debug build): let tool calls finish.
               MCP_TOOL_TIMEOUT=os.environ.get("MCP_TOOL_TIMEOUT", "600000"),
               MCP_TIMEOUT=os.environ.get("MCP_TIMEOUT", "60000"))
    cli = Cli(bins / "ryolune-cli", env)
    song_path = work / "song.ryolune"
    FIXTURES[job["fixture"]](cli, song_path, work)
    before_path = work / "before.ryolune"
    shutil.copyfile(song_path, before_path)
    before = Song(cli, before_path)
    mcp_config = work / "mcp.json"
    mcp_config.write_text(json.dumps({"mcpServers": {"ryolune": {
        "command": str(bins / "ryolune-mcp"), "args": ["--file", str(song_path)],
        "env": {k: env[k] for k in ("RYOLUNE_SETTINGS", "RYOLUNE_DATA_DIR", "RYOLUNE_CONTROL", "LSUITE_HOME")},
    }}}))
    prompt = job["prompt"].replace("{work}", str(work))
    agent = run_agent(claude, args.model, prompt, mcp_config, work, env, args.timeout, args.budget)
    song = Song(cli, song_path)
    ctx = {"before": before, "tools": agent["tools"], "reply": agent["reply"], "work": str(work)}
    checks = []
    for fn in job["checks"]:
        try:
            passed, detail = fn(song, ctx)
        except Exception as e:  # a check that cannot run fails
            passed, detail = False, f"error: {e}"
            if args.verbose:
                traceback.print_exc()
        checks.append({"check": fn.check_name, "passed": passed, "detail": str(detail)[:300]})
    passed = all(c["passed"] for c in checks) and not agent["error"]
    return {"job": job["id"], "skill": job["skill"], "passed": passed,
            "score": round(sum(c["passed"] for c in checks) / len(checks), 3),
            "checks": checks, "agent": {k: v for k, v in agent.items()}, "work": str(work)}


def record(results, args):
    path = HERE / "RESULTS.md"
    date = datetime.date.today().isoformat()
    jobs = len(results)
    passed = sum(r["passed"] for r in results)
    checks = [c for r in results for c in r["checks"]]
    cost = sum(r["agent"]["costUsd"] or 0 for r in results)
    lines = [f"\n### {date} · {args.model} · {passed}/{jobs} jobs passed · "
             f"{sum(c['passed'] for c in checks)}/{len(checks)} checks · ${cost:.2f}\n",
             "\n| Job | Skill | Result | Checks | Tool calls | Time | Notes |\n| --- | --- | --- | --- | --- | --- | --- |\n"]
    for r in results:
        failed = [f"{c['check']} ({c['detail']})" for c in r["checks"] if not c["passed"]]
        notes = "; ".join(failed) or "all checks passed"
        if r["agent"]["error"]:
            notes = f"agent error: {r['agent']['error'][:120]}; " + notes
        lines.append(f"| {r['job']} | {r['skill']} | {'pass' if r['passed'] else 'FAIL'} | "
                     f"{sum(c['passed'] for c in r['checks'])}/{len(r['checks'])} | {len(r['agent']['tools'])} | "
                     f"{r['agent']['seconds']}s | {notes.replace('|', '/')} |\n")
    with path.open("a") as f:
        f.writelines(lines)


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--jobs", help="comma-separated job ids (default: all)")
    p.add_argument("--model", default=os.environ.get("RYOLUNE_EVAL_MODEL", "sonnet"))
    p.add_argument("--bin-dir")
    p.add_argument("--claude")
    p.add_argument("--timeout", type=int, default=900, help="seconds per job")
    p.add_argument("--budget", type=float, default=3.0, help="max USD per job (Claude Code --max-budget-usd)")
    p.add_argument("--record", action="store_true", help="append the run to evals/RESULTS.md")
    p.add_argument("--keep", action="store_true", help="keep the work folders")
    p.add_argument("--list", action="store_true")
    p.add_argument("--verbose", action="store_true")
    args = p.parse_args()
    if args.list:
        for j in JOBS:
            print(f"{j['id']:<18} {j['fixture']:<11} {j['prompt']}")
        return
    wanted = set(args.jobs.split(",")) if args.jobs else None
    jobs = [j for j in JOBS if wanted is None or j["id"] in wanted]
    if wanted and len(jobs) != len(wanted):
        sys.exit(f"Unknown jobs: {sorted(wanted - {j['id'] for j in jobs})}")
    bins, claude = find_bins(args.bin_dir), find_claude(args.claude)
    root = Path(tempfile.mkdtemp(prefix="ryolune-evals-"))
    results = []
    for job in jobs:
        print(f"→ {job['id']} …", flush=True)
        try:
            r = run_job(job, args, bins, claude, root)
        except Exception as e:
            traceback.print_exc()
            r = {"job": job["id"], "skill": job["skill"], "passed": False, "score": 0,
                 "checks": [{"check": "the job ran", "passed": False, "detail": str(e)[:300]}],
                 "agent": {"tools": [], "seconds": 0, "costUsd": None, "error": str(e)[:300]}, "work": ""}
        results.append(r)
        mark = "pass" if r["passed"] else "FAIL"
        print(f"  {mark} {sum(c['passed'] for c in r['checks'])}/{len(r['checks'])} checks, "
              f"{len(r['agent']['tools'])} tool calls, {r['agent']['seconds']}s, ${r['agent']['costUsd'] or 0:.2f}")
        for c in r["checks"]:
            print(f"    {'✓' if c['passed'] else '✗'} {c['check']}: {c['detail']}")
    out = HERE / "results"
    out.mkdir(exist_ok=True)
    stamp = datetime.datetime.now().strftime("%Y-%m-%d-%H%M%S")
    (out / f"{stamp}.json").write_text(json.dumps({"model": args.model, "results": results}, indent=2))
    passed = sum(r["passed"] for r in results)
    print(f"\n{passed}/{len(results)} jobs passed · results in evals/results/{stamp}.json")
    if args.record:
        record(results, args)
    if not args.keep:
        shutil.rmtree(root, ignore_errors=True)
    else:
        print(f"work folders kept in {root}")


if __name__ == "__main__":
    main()
