"""Fail-closed minimal npm graph equality against the two existing pnpm locks."""
import json
import re
from pathlib import Path

HERE = Path(__file__).resolve().parent
BROWSER = HERE.parent

def verify():
    if not __debug__:
        raise ValueError("Lock checks must not be disabled")
    manifest = json.loads((HERE / 'package.json').read_text())
    lock = json.loads((HERE / 'package-lock.json').read_text())
    sources = [(BROWSER / 'pnpm-lock.yaml').read_text(), (BROWSER / 'harness-fixture-runtime/pnpm-lock.yaml').read_text()]
    expected_direct = {'playwright-core': json.loads((BROWSER / 'package.json').read_text())['dependencies']['playwright-core'],
                       **json.loads((BROWSER / 'harness-fixture-runtime/package.json').read_text())['dependencies']}
    assert manifest['dependencies'] == expected_direct
    assert manifest.get('scripts') is None and manifest['private'] is True
    assert lock['lockfileVersion'] == 3
    assert lock['packages']['']['dependencies'] == expected_direct
    def package(name, version):
        key = re.escape(name + '@' + version)
        matches = []
        for source in sources:
            section = source.split('\npackages:\n', 1)[1].split('\nsnapshots:\n', 1)[0]
            block = re.search(r'^  ' + key + r':\n((?:    .*\n|\n)*)', section, re.M)
            if block:
                integrity = re.search(r'resolution: \{integrity: (sha512-[A-Za-z0-9+/=]+)\}', block[1])
                assert integrity, 'Missing source integrity'
                snapshot = source.split('\nsnapshots:\n', 1)[1]
                record = re.search(r'^  ' + key + r'(?:\([^\n]+\))?: (?:\{\})\s*$|^  ' + key + r'(?:\([^\n]+\))?:\n((?:    .*\n|\n)*)', snapshot, re.M)
                assert record, 'Missing source snapshot'
                lines = [line for line in (record[1] or '').splitlines() if line.strip()]
                dependencies = {}
                if lines:
                    assert lines.pop(0) == '    dependencies:', 'Unsupported snapshot section'
                    for line in lines:
                        dependency = re.fullmatch(r'      ([a-z][a-z0-9-]*): ([0-9]+\.[0-9]+\.[0-9]+)(?:\([^\n]*\))?', line)
                        assert dependency, 'Unsupported snapshot dependency'
                        assert dependency[1] not in dependencies
                        dependencies[dependency[1]] = dependency[2]
                matches.append((integrity[1], dependencies))
        assert len(matches) == 1, 'Ambiguous dependency source'
        return matches[0]
    pending = list(expected_direct.items())
    verified = {}
    while pending:
        name, version = pending.pop()
        if name in verified:
            assert verified[name] == version
            continue
        integrity, deps = package(name, version)
        entry = lock['packages']['node_modules/' + name]
        assert entry['version'] == version and entry['integrity'] == integrity
        assert entry['resolved'] == f'https://registry.npmjs.org/{name}/-/{name}-{version}.tgz'
        assert entry.get('dependencies', {}) == deps
        assert not any(key in entry for key in ['hasInstallScript', 'link', 'optionalDependencies'])
        verified[name] = version
        pending.extend(deps.items())
    assert set(lock['packages']) == {''} | {'node_modules/' + name for name in verified}
    assert len(verified) == 6, 'Review changed minimal fixture dependency graph explicitly'
    return verified

if __name__ == '__main__':
    try:
        verified = verify()
    except (AssertionError, KeyError, ValueError, IndexError, OSError, TypeError):
        raise SystemExit('Minimal dependency lock differs from repository pins; no install allowed')
    print(f'Minimal test lock verified: {len(verified)} exact versions and integrity records')
