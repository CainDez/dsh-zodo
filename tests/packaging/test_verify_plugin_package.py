import importlib.util
from pathlib import Path
import stat
import struct
from tempfile import TemporaryDirectory
import unittest
import warnings
from zipfile import ZipFile, ZipInfo

SCRIPT = Path(__file__).resolve().parents[2] / "scripts/verify-plugin-package.py"
SPEC = importlib.util.spec_from_file_location("verify_package", SCRIPT)
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


def pe_header(machine=0x8664):
    header = bytearray(112)
    header[:2] = b"MZ"
    struct.pack_into("<I4sH", header, 60, 64, b"PE\0\0", machine)
    struct.pack_into("<HHH", header, 84, 24, 2, 0x20B)
    return bytes(header)


def elf_header(machine=62, elf_type=3):
    header = bytearray(120)
    header[:7] = b"\x7fELF\x02\x01\x01"
    struct.pack_into("<HHIQQ", header, 16, elf_type, machine, 1, 0x400000, 64)
    struct.pack_into("<HHH", header, 52, 64, 56, 1)
    return bytes(header)


class VerifyPluginPackage(unittest.TestCase):
    def setUp(self):
        tmp = TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.package = Path(tmp.name) / "fixture.zip"
        self.core = Path(tmp.name) / "zodo.exe"
        self.core.write_bytes(pe_header())
        self.entries = {name: self.core.read_bytes() if name == MODULE.CORE
                        else (MODULE.SOURCE / name).read_bytes()
                        for name in MODULE.source_files(MODULE.SOURCE)}

    def pack(self, *extra):
        with ZipFile(self.package, "w") as zip_file:
            for name, data in [*self.entries.items(), *extra]:
                zip_file.writestr(name, data)

    def test_accept_matching_contents_with_optional_parent_directories(self):
        self.pack(("bin/", b""), ("bin/win32-x64/", b""), ("locale/", b""))
        self.assertRegex(MODULE.verify(self.package, self.core), r"^[a-f0-9]{64}$")

    def test_reject_missing_and_unexpected_members(self):
        del self.entries["client.js"]
        self.pack(("locale/test-db.sqlite", b"synthetic"))
        with self.assertRaisesRegex(ValueError, "Extra or missing"):
            MODULE.verify(self.package, self.core)

    def test_reject_duplicate_members(self):
        with warnings.catch_warnings():
            warnings.simplefilter("ignore", UserWarning)
            self.pack(("client.js", self.entries["client.js"]))
        with self.assertRaisesRegex(ValueError, "Duplicate ZIP"):
            MODULE.verify(self.package, self.core)

    def test_reject_stale_bundled_core(self):
        self.entries[MODULE.CORE] = self.core.read_bytes()[:-1] + b"X"
        self.pack()
        with self.assertRaisesRegex(ValueError, MODULE.CORE):
            MODULE.verify(self.package, self.core)

    def test_reject_stale_bundled_client(self):
        self.entries["client.js"] += b"\n"
        self.pack()
        with self.assertRaisesRegex(ValueError, "client.js"):
            MODULE.verify(self.package, self.core)

    def test_reject_core_for_other_architecture(self):
        self.core.write_bytes(pe_header(machine=0xAA64))
        self.pack()
        with self.assertRaisesRegex(ValueError, "not a Windows x64"):
            MODULE.verify(self.package, self.core)


class VerifyLinuxPluginPackage(unittest.TestCase):
    def setUp(self):
        tmp = TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.package = Path(tmp.name) / "fixture.zip"
        self.core = Path(tmp.name) / "zodo"
        self.core.write_bytes(elf_header())
        self.target = "linux-x64"

    def pack(self, mode=stat.S_IFREG | 0o755, bundled=None, windows=False, system=3):
        with ZipFile(self.package, "w") as archive:
            for name in MODULE.source_files(MODULE.SOURCE, self.target):
                if name == MODULE.LINUX_CORE:
                    entry = ZipInfo(name)
                    entry.create_system = system
                    entry.external_attr = mode << 16
                    archive.writestr(entry, self.core.read_bytes() if bundled is None else bundled)
                else:
                    archive.writestr(name, (MODULE.SOURCE / name).read_bytes())
            if windows:
                archive.writestr(MODULE.CORE, pe_header())

    def verify(self):
        return MODULE.verify(self.package, self.core, target=self.target)

    def test_accept_matching_elf_and_explicit_mode(self):
        self.pack()
        self.assertRegex(self.verify(), r"^[a-f0-9]{64}$")

    def test_reject_other_architecture_and_relocatable_elf(self):
        for header in [elf_header(machine=183), elf_header(elf_type=1)]:
            with self.subTest(header=header):
                self.core.write_bytes(header)
                self.pack()
                with self.assertRaisesRegex(ValueError, "not a Linux x64"):
                    self.verify()

    def test_reject_wrong_permissions_and_non_unix_metadata(self):
        for mode, system in [(stat.S_IFREG | 0o644, 3), (stat.S_IFREG | 0o755, 0),
                             (stat.S_IFIFO | 0o755, 3)]:
            with self.subTest(mode=mode, system=system):
                self.pack(mode=mode, system=system)
                with self.assertRaisesRegex(ValueError, "regular executable"):
                    self.verify()

    def test_reject_other_platform_in_archive(self):
        self.pack(windows=True)
        with self.assertRaisesRegex(ValueError, "Extra or missing"):
            self.verify()

    def test_reject_nonmatching_elf(self):
        self.pack(bundled=self.core.read_bytes()[:-1] + b"X")
        with self.assertRaisesRegex(ValueError, "ZIP bytes differ"):
            self.verify()


if __name__ == "__main__":
    unittest.main()
