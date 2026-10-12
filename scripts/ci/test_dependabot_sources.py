"""Exercise Cargo-source repair and dispatch outside a Git checkout."""

import os
from pathlib import Path
import re
import subprocess
import tempfile
import textwrap
import unittest


class DependabotSourcesTests(unittest.TestCase):
    def test_dispatch_after_changed_or_already_repaired_sources(self):
        workflow = (
            Path(__file__).resolve().parents[2]
            / ".github/workflows/dependabot-cargo-sources.yml"
        ).read_text()
        repair = re.search(r"        run: \|\n((?:          .*\n|\n)+)", workflow)[1]
        dispatch = re.search(r"        run: (gh workflow run .*?)\n", workflow)[1]
        for changed in (False, True):
            with self.subTest(changed=changed), tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp)
                (root / "trusted/scripts").mkdir(parents=True)
                (root / "trusted/scripts/generate-cargo-sources.py").write_text("pass\n")
                (root / "update").mkdir()
                bindir = root / "bin"
                bindir.mkdir()
                git = bindir / "git"
                git.write_text(
                    "#!/bin/sh\n"
                    'printf "%s\\n" "$*" >> "$CALL_LOG"\n'
                    'if [ "$*" = "-C update diff --quiet -- flatpak/cargo-sources.json" ]; then\n'
                    '  exit "$SOURCE_CHANGED"\n'
                    "fi\n"
                )
                gh = bindir / "gh"
                gh.write_text(
                    "#!/bin/sh\n"
                    'printf "%s\\n" "$*" >> "$CALL_LOG"\n'
                    'test "$*" = "workflow run flatpak.yml --repo nedrichards/pinpoint-ng '
                    '--ref dependabot/cargo/libc-0.2.190"\n'
                )
                git.chmod(0o755)
                gh.chmod(0o755)
                log = root / "calls"
                env = {
                    **os.environ,
                    "PATH": f"{bindir}:{os.environ['PATH']}",
                    "SOURCE_CHANGED": str(int(changed)),
                    "CALL_LOG": str(log),
                    "GITHUB_REPOSITORY": "nedrichards/pinpoint-ng",
                    "TARGET_BRANCH": "dependabot/cargo/libc-0.2.190",
                }
                for script in (textwrap.dedent(repair), dispatch):
                    subprocess.run(["bash", "-e", "-c", script], cwd=root, env=env, check=True)
                calls = log.read_text()
                self.assertIn("workflow run flatpak.yml --repo nedrichards/pinpoint-ng", calls)
                self.assertEqual("-C update push origin" in calls, changed)


if __name__ == "__main__":
    unittest.main()
