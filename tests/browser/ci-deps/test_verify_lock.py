import importlib.util
import json
from pathlib import Path
import shutil
import tempfile
import unittest

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location('minimal_lock', HERE / 'verify-lock.py')
validator = importlib.util.module_from_spec(spec)
spec.loader.exec_module(validator)

class MinimalLockTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.browser = Path(self.temp.name)
        for relative in ['package.json', 'pnpm-lock.yaml', 'harness-fixture-runtime/package.json', 'harness-fixture-runtime/pnpm-lock.yaml', 'ci-deps/package.json', 'ci-deps/package-lock.json']:
            target = self.browser / relative
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(HERE.parent / relative, target)
        validator.HERE, validator.BROWSER = self.browser / 'ci-deps', self.browser
    def tearDown(self):
        self.temp.cleanup()
    def alter(self, fn):
        path = validator.HERE / 'package-lock.json'
        value = json.loads(path.read_text())
        fn(value)
        path.write_text(json.dumps(value))
    def test_matches_six_existing_pins(self):
        self.assertEqual(len(validator.verify()), 6)
    def test_rejects_integrity_drift(self):
        self.alter(lambda v: v['packages']['node_modules/react'].update(integrity='sha512-invalid'))
        with self.assertRaises(AssertionError): validator.verify()
    def test_rejects_nonregistry_or_extra_dependency(self):
        self.alter(lambda v: v['packages']['node_modules/react'].update(resolved='https://other.invalid/react.tgz'))
        with self.assertRaises(AssertionError): validator.verify()
    def test_rejects_omitted_transitive_edge(self):
        self.alter(lambda v: v['packages']['node_modules/react-dom'].update(dependencies={}))
        with self.assertRaises(AssertionError): validator.verify()
    def test_rejects_added_package(self):
        self.alter(lambda v: v['packages'].update({'node_modules/extra': {'version': '1.0.0'}}))
        with self.assertRaises(AssertionError): validator.verify()

if __name__ == '__main__': unittest.main()
