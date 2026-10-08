#!/usr/bin/env python3
"""Exercise release publication without contacting GitHub or changing a real tag."""
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest
import zipfile

ROOT = Path(__file__).resolve().parents[2]
sys.dont_write_bytecode = True
PACKAGE_SPEC = importlib.util.spec_from_file_location("package_portable", ROOT / "scripts/package-portable.py")
PACKAGE = importlib.util.module_from_spec(PACKAGE_SPEC)
PACKAGE_SPEC.loader.exec_module(PACKAGE)
# Linux only while lsuite is in beta (macOS and Windows are coming soon).
ASSETS = [
    "ryolune-linux-x86_64.zip", "ryolune-linux-x86_64.tar.gz",
    "ryolune-linux-x86_64", "ryolune-Afterglow-demo.zip",
]
FAKE_GH = '''#!/usr/bin/env python3
import json, os, pathlib, sys
args = sys.argv[1:]
root = pathlib.Path(os.environ["RELEASE_FIXTURE"])
state = root / "release-state"
with (root / "gh-calls").open("a") as f:
    f.write(json.dumps(args) + "\\n")
command = args[1]
if command == "view":
    if not state.exists(): sys.exit(1)
    draft = state.read_text() == "draft"
    if "--jq" in args: print(str(draft).lower())
    else:
        names = os.environ["EXPECTED_ASSETS"].split(";")
        print(json.dumps({"isDraft": draft, "assets": [{"name": n} for n in names]}))
elif command == "create": state.write_text("draft")
elif command == "edit":
    if "--draft=false" in args: state.write_text("published")
elif command == "upload":
    if state.read_text() != "draft": raise SystemExit("Cannot overwrite a published release")
elif command == "download":
    pathlib.Path(args[args.index("--output") + 1]).write_bytes((root / "published-sums").read_bytes())
else: raise SystemExit("Unexpected gh command: " + str(args))
'''
FAKE_GIT = '''#!/usr/bin/env python3
import os, sys
if sys.argv[1] == "rev-parse": print("a" * 40)
elif sys.argv[1] == "ls-remote":
    print(os.environ.get("REMOTE_SHA", "a" * 40) + "\\trefs/tags/v0.3.1")
else: raise SystemExit("Unexpected git command")
'''


class ReleaseWorkflow(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        (self.root / "scripts").mkdir()
        (self.root / "bin").mkdir()
        (self.root / "dist").mkdir()
        (self.root / "docs/releases").mkdir(parents=True)
        for name in ["verify-release.sh", "publish-release.sh"]:
            shutil.copyfile(ROOT / "scripts" / name, self.root / "scripts" / name)
        (self.root / "Cargo.toml").write_text('[workspace.package]\nversion = "0.3.1"\n')
        (self.root / "docs/releases/0.3.1.md").write_text("Release notes\n")
        sums = []
        for name in ASSETS:
            payload = name.encode()
            (self.root / "dist" / name).write_bytes(payload)
            sums.append(hashlib.sha256(payload).hexdigest() + "  " + name)
        (self.root / "dist/SHA256SUMS").write_text("\n".join(sums) + "\n")
        (self.root / "dist/SHA256SUMS.sig").write_text("ryolune-ed25519 c2lnbmF0dXJl\n")
        for name, script in [("gh", FAKE_GH), ("git", FAKE_GIT)]:
            path = self.root / "bin" / name
            path.write_text(script)
            path.chmod(0o755)
        self.env = dict(os.environ, PATH=str(self.root / "bin") + os.pathsep + os.environ["PATH"],
                        GITHUB_REF_TYPE="tag", GITHUB_REF_NAME="v0.3.1",
                        RELEASE_FIXTURE=str(self.root), EXPECTED_ASSETS=";".join(ASSETS + ["SHA256SUMS", "SHA256SUMS.sig"]))

    def run_release(self):
        return subprocess.run(["bash", "scripts/publish-release.sh"], cwd=self.root,
                              env=self.env, capture_output=True, text=True)

    def calls(self):
        path = self.root / "gh-calls"
        return [json.loads(line)[1] for line in path.read_text().splitlines()] if path.exists() else []

    def test_new_release_stays_a_draft_for_publish_build(self):
        result = self.run_release()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.calls(), ["view", "create", "view", "upload", "edit"])
        # The builds are not public: the draft is handed to scripts/publish-build.sh.
        self.assertEqual((self.root / "release-state").read_text(), "draft")
        raw = (self.root / "gh-calls").read_text()
        self.assertNotIn("--draft=false", raw)
        self.assertNotIn("--latest", raw)
        self.assertIn("publish-build.sh 0.3.1", result.stdout)

    def test_draft_can_be_completed_without_creating_another_release(self):
        (self.root / "release-state").write_text("draft")
        result = self.run_release()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertNotIn("create", self.calls())
        self.assertEqual(self.calls()[-2:], ["upload", "edit"])

    def test_published_release_with_same_manifest_is_a_read_only_noop(self):
        (self.root / "release-state").write_text("published")
        shutil.copyfile(self.root / "dist/SHA256SUMS", self.root / "published-sums")
        result = self.run_release()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.calls(), ["view", "download"])

    def test_published_release_with_changed_bytes_is_not_overwritten(self):
        (self.root / "release-state").write_text("published")
        (self.root / "published-sums").write_text("different payload\n")
        result = self.run_release()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("already published with different checksums", result.stderr)
        self.assertEqual(self.calls(), ["view", "download"])

    def test_wrong_ref_moved_tag_missing_notes_and_asset_fail_before_github(self):
        cases = ["branch", "moved-tag", "missing-notes", "missing-asset", "missing-demo", "missing-signature"]
        for case in cases:
            with self.subTest(case=case):
                original_env = self.env.copy()
                moved = None
                if case == "branch": self.env["GITHUB_REF_TYPE"] = "branch"
                if case == "moved-tag": self.env["REMOTE_SHA"] = "b" * 40
                if case == "missing-notes": moved = self.root / "docs/releases/0.3.1.md"
                if case == "missing-asset": moved = self.root / "dist" / ASSETS[0]
                if case == "missing-demo": moved = self.root / "dist/ryolune-Afterglow-demo.zip"
                if case == "missing-signature": moved = self.root / "dist/SHA256SUMS.sig"
                if moved: moved.rename(moved.with_suffix(".held"))
                result = self.run_release()
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(self.calls(), [])
                if moved: moved.with_suffix(".held").rename(moved)
                self.env = original_env


FAKE_GH_BUILDS = '''#!/usr/bin/env python3
import json, os, pathlib, shutil, sys
args = sys.argv[1:]
root = pathlib.Path(os.environ["RELEASE_FIXTURE"])
with (root / "gh-calls").open("a") as f:
    f.write(json.dumps(args) + "\\n")
repo = args[args.index("--repo") + 1]
tag = args[2]
release = root / "repos" / repo.replace("/", "_") / tag
command = args[1]
if command == "view":
    if not release.exists(): sys.exit(1)
    if "--jq" in args:
        field = args[args.index("--jq") + 1]
        print({".isDraft": (release / "state").read_text() == "draft" and "true" or "false",
               ".body": (release / "notes").read_text()}[field])
    else: print(json.dumps({"tagName": tag}))
elif command == "download":
    files = release / "files"
    if "--pattern" in args:
        name = args[args.index("--pattern") + 1]
        shutil.copyfile(files / name, args[args.index("--output") + 1])
    else:
        target = pathlib.Path(args[args.index("--dir") + 1])
        for f in files.iterdir(): shutil.copyfile(f, target / f.name)
elif command == "create":
    if release.exists(): raise SystemExit("exists")
    (release / "files").mkdir(parents=True)
    (release / "state").write_text("published")
    (release / "notes").write_text(pathlib.Path(args[args.index("--notes-file") + 1]).read_text())
    (release / "title").write_text(args[args.index("--title") + 1])
    for f in args:
        p = pathlib.Path(f)
        if p.is_file() and p.parent.name == "files": shutil.copyfile(p, release / "files" / p.name)
elif command == "delete":
    shutil.rmtree(release)
else: raise SystemExit("Unexpected gh command: " + str(args))
'''


class PublishBuild(unittest.TestCase):
    """scripts/publish-build.sh moves a complete draft to lsuite-builds."""

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        (self.root / "bin").mkdir()
        path = self.root / "bin" / "gh"
        path.write_text(FAKE_GH_BUILDS)
        path.chmod(0o755)
        self.draft = self.root / "repos" / "ludovic111_ryolune" / "v0.16.0"
        (self.draft / "files").mkdir(parents=True)
        (self.draft / "state").write_text("draft")
        (self.draft / "notes").write_text("Notes of 0.16.0")
        sums = []
        for name in ASSETS:
            (self.draft / "files" / name).write_bytes(name.encode())
            sums.append(hashlib.sha256(name.encode()).hexdigest() + "  " + name)
        (self.draft / "files/SHA256SUMS").write_text("\n".join(sums) + "\n")
        (self.draft / "files/SHA256SUMS.sig").write_text("ryolune-ed25519 c2lnbmF0dXJl\n")
        self.env = dict(os.environ, PATH=str(self.root / "bin") + os.pathsep + os.environ["PATH"],
                        RELEASE_FIXTURE=str(self.root))

    def publish(self, version="0.16.0"):
        return subprocess.run(["bash", str(ROOT / "scripts/publish-build.sh"), version],
                              env=self.env, capture_output=True, text=True)

    def test_the_draft_becomes_a_private_build_and_is_deleted(self):
        result = self.publish()
        self.assertEqual(result.returncode, 0, result.stderr)
        build = self.root / "repos" / "ludovic111_lsuite-builds" / "ryolune-v0.16.0"
        self.assertEqual(sorted(p.name for p in (build / "files").iterdir()),
                         sorted(ASSETS + ["SHA256SUMS", "SHA256SUMS.sig"]))
        self.assertEqual((build / "notes").read_text().strip(), "Notes of 0.16.0")
        self.assertEqual((build / "title").read_text(), "ryolune 0.16.0")
        self.assertFalse(self.draft.exists())

    def test_a_public_release_or_a_damaged_draft_is_refused(self):
        (self.draft / "state").write_text("published")
        self.assertNotEqual(self.publish().returncode, 0)
        (self.draft / "state").write_text("draft")
        (self.draft / "files" / ASSETS[0]).write_bytes(b"changed")
        self.assertNotEqual(self.publish().returncode, 0)
        self.assertTrue(self.draft.exists())
        self.assertNotEqual(self.publish("latest").returncode, 0)


class PortablePackaging(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)

    def test_binary_archives_have_exact_updater_names(self):
        for kind, suffix in [("linux", ""), ("windows", ".exe")]:
            expected = [name + suffix for name in ["ryolune", "ryolune-cli", "ryolune-mcp"]]
            for name in expected:
                (self.root / name).write_bytes(b"binary")
            output = self.root / (kind + ".zip")
            PACKAGE.package(self.root, output, kind)
            with zipfile.ZipFile(output) as archive:
                self.assertEqual(archive.namelist(), expected)
                self.assertIsNone(archive.testzip())
            with self.assertRaises(FileExistsError):
                PACKAGE.package(self.root, output, kind)

    def test_demo_requires_complete_stock_verification_and_matching_audio(self):
        for name in ["Afterglow.ryolune", "Afterglow.wav", "Afterglow.mid"]:
            (self.root / name).write_bytes(name.encode())
        report = {"instrument": "stock", "effect": "stock", "mode": "headless",
                  "validation": "Valid session: Afterglow",
                  "sha256": hashlib.sha256((self.root / "Afterglow.wav").read_bytes()).hexdigest()}
        (self.root / "verification.json").write_text(json.dumps(report))
        output = self.root / "demo.zip"
        PACKAGE.package(self.root, output, "demo")
        with zipfile.ZipFile(output) as archive:
            self.assertEqual(archive.namelist(), ["Afterglow.ryolune", "Afterglow.wav", "Afterglow.mid", "verification.json"])
        for change in [{"instrument": "external"}, {"mode": "live"}, {"sha256": "wrong"}, {"validation": ""}]:
            (self.root / "verification.json").write_text(json.dumps(dict(report, **change)))
            with self.assertRaises(ValueError):
                PACKAGE.package(self.root, self.root / "rejected.zip", "demo")
            self.assertFalse((self.root / "rejected.zip").exists())


if __name__ == "__main__":
    unittest.main()
