"""Safety-contract tests: no Core process or external request is started."""
import ast
import hashlib
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.dont_write_bytecode = True
REPO = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(REPO / "tools"))
import run_isolated_integration as runner


class SafetyTests(unittest.TestCase):
    def setUp(self):
        parent = REPO / "tests" / "integration" / "runs"
        parent.mkdir(parents=True, exist_ok=True)
        self.temp = tempfile.TemporaryDirectory(prefix="guard-test-", dir=parent)
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        self.binary = (REPO / "target" / "release" / "zodo.exe").resolve()

    def test_all_executable_legacy_sources_match_review(self):
        self.assertEqual(set(runner.CASES), set(runner.REVIEWED_SHA256))
        for name, expected in runner.REVIEWED_SHA256.items():
            source = (REPO / "tools" / name).read_text(encoding="utf-8")
            self.assertEqual(hashlib.sha256(source.encode()).hexdigest(), expected, name)

    def test_all_legacy_tests_are_classified(self):
        actual = {p.name for p in (REPO / "tools").glob("test_*.py")}
        self.assertEqual(set(runner.CASES) | set(runner.SKIPPED), actual)
        self.assertFalse(set(runner.CASES) & set(runner.SKIPPED))

    def test_environment_excludes_secrets_proxy_and_live_configuration(self):
        with patch.dict(os.environ, {"DEEPSEEK_API_KEY":"never-copy", "ZODO_DATA_DIR":"live", "HTTPS_PROXY":"https://external.invalid", "ZODO_CALENDAR_WRITE_URL":"https://external.invalid"}):
            env = runner.safe_env(self.root)
        for key in ("DEEPSEEK_API_KEY", "ZODO_DATA_DIR", "HTTPS_PROXY", "ZODO_CALENDAR_WRITE_URL"):
            self.assertNotIn(key, env)
        for key in ("HOME", "USERPROFILE", "APPDATA", "LOCALAPPDATA", "TEMP", "TMP"):
            self.assertTrue(Path(env[key]).is_relative_to(self.root))

    def test_transform_removes_hardcoded_root_port_and_local_model(self):
        source = 'ROOT = pathlib.Path(r"D:\\projects\\zodo_dsh")\nZ = ROOT / "target" / "release" / "zodo.exe"\nPORT = 39091\nurl = "http://127.0.0.1:11434/v1"\nserver = ThreadingHTTPServer(("127.0.0.1", PORT), Stub)'
        tree = runner.Relocate(self.root, self.binary, "http://127.0.0.1:12345/v1").visit(ast.parse(source))
        rendered = ast.unparse(ast.fix_missing_locations(tree))
        self.assertNotIn("39091", rendered)
        self.assertNotIn("11434", rendered)
        self.assertIn("PORT = 0", rendered)
        self.assertIn("PORT = server.server_address[1]", rendered)

    def test_core_outside_fixture_is_denied(self):
        with runner.owned_processes(self.root, self.binary, "http://127.0.0.1:1/v1"):
            with self.assertRaisesRegex(RuntimeError, "outside unique"):
                runner.subprocess.Popen([str(self.binary), "--data-dir", str(REPO), "serve", "--port", "0"])

    def test_implicit_data_foreign_executable_and_fixed_port_are_denied(self):
        with runner.owned_processes(self.root, self.binary, "http://127.0.0.1:1/v1"):
            for argv in ([str(self.binary), "serve"], ["other.exe", "--data-dir", str(self.root/"core"), "serve"], [str(self.binary), "--data-dir", str(self.root/"core"), "serve", "--port", "3181"]):
                with self.assertRaises(RuntimeError):
                    runner.subprocess.Popen(argv)

    def test_restore_external_backup_is_denied(self):
        with runner.owned_processes(self.root, self.binary, "http://127.0.0.1:1/v1"):
            with self.assertRaisesRegex(RuntimeError, "backup must belong"):
                runner.subprocess.Popen([str(self.binary), "--data-dir", str(self.root/"core"), "restore", str(REPO/"never-read.sqlite3")])

    def test_http_live_local_and_external_targets_are_denied(self):
        with runner.owned_processes(self.root, self.binary, "http://127.0.0.1:1/v1"):
            for url in ("http://127.0.0.1:3181/v1", "https://api.deepseek.com/v1"):
                with self.assertRaisesRegex(RuntimeError, "non-owned"):
                    runner.urllib.request.urlopen(url)


if __name__ == "__main__":
    unittest.main()
