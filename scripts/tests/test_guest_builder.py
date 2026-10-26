"""Pinned SDK and isolated own-source builds, without executing a real SDK."""
import hashlib
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

PIN = '14d5ee97e73347c973f1f57b68b79ec08c9e77f2'
SCRIPT = Path(__file__).resolve().parents[1] / 'build-nxdk-sample.sh'


class GuestBuilderTest(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='obsidian builder ')
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        (self.root / 'scripts').mkdir()
        shutil.copyfile(SCRIPT, self.root / 'scripts/build-nxdk-sample.sh')
        self.source = self.root / 'homebrew/file-read'
        self.source.mkdir(parents=True)
        (self.source / 'Makefile').write_text('guest-makefile\n')
        (self.source / 'main.c').write_text('guest-source\n')
        self.sdk = self.root / 'external sdk'
        (self.sdk / 'bin').mkdir(parents=True)
        binary = self.root / 'fake-bin'
        binary.mkdir()
        commands = {
            'git': '''#!/usr/bin/env python3
import os, sys
a = sys.argv[1:]
if 'rev-parse' in a: print(os.environ['TEST_REVISION'])
elif 'diff' in a: sys.exit(int(os.environ.get('TEST_DIRTY', '0')))
''',
            'make': '''#!/usr/bin/env python3
import os, pathlib, sys
a = sys.argv[1:]; source = pathlib.Path(a[a.index('-C') + 1])
assert source != pathlib.Path(os.environ['TEST_SOURCE'])
assert (source / 'main.c').read_text() == 'guest-source\\n'
assert os.environ['NXDK_DIR'] == os.environ['TEST_SDK']
assert os.environ['SOURCE_DATE_EPOCH'] == '1704067200'
(source / 'bin').mkdir()
(source / 'bin/default.xbe').write_bytes(b'xbe-built-from-guest')
(source / 'main.exe').write_bytes(b'pe-built-from-guest')
''',
        }
        for name, text in commands.items():
            file = binary / name
            file.write_text(text)
            file.chmod(0o755)
        self.env = dict(os.environ, PATH=str(binary) + os.pathsep + os.environ['PATH'],
                        NXDK_BUILD_DIR=str(self.sdk), TEST_REVISION=PIN,
                        TEST_SOURCE=str(self.source), TEST_SDK=str(self.sdk))

    def run_builder(self, sample='file-read', destination='owned-output', **env):
        return subprocess.run(['bash', str(self.root / 'scripts/build-nxdk-sample.sh'),
                               sample, destination], env=dict(self.env, **env),
                              capture_output=True, text=True, timeout=20)

    def test_own_source_is_copied_and_sdk_pin_source_hashes_are_recorded(self):
        result = self.run_builder()
        self.assertEqual(result.returncode, 0, result.stderr)
        output = self.root / 'target/owned-output'
        self.assertEqual((output / 'file-read-raw.xbe').read_bytes(), b'xbe-built-from-guest')
        self.assertEqual((output / 'file-read.exe').read_bytes(), b'pe-built-from-guest')
        self.assertEqual(sorted(p.name for p in self.source.iterdir()), ['Makefile', 'main.c'])
        self.assertEqual(list(self.sdk.iterdir()), [self.sdk / 'bin'])
        manifest = (output / 'build-file-read-manifest.txt').read_text()
        self.assertIn('nxdk=' + PIN, manifest)
        for name in ['Makefile', 'main.c']:
            self.assertIn(hashlib.sha256((self.source / name).read_bytes()).hexdigest(), manifest)
        sums = (output / 'SHA256SUMS-file-read').read_text()
        self.assertIn(hashlib.sha256(b'xbe-built-from-guest').hexdigest(), sums)
        self.assertEqual(len(list(output.glob('source-*'))), 1)

    def test_unpinned_or_dirty_sdk_is_rejected_before_any_build_output(self):
        for env in [dict(TEST_REVISION='wrong'), dict(TEST_DIRTY='1')]:
            with self.subTest(env=env):
                result = self.run_builder(**env)
                self.assertNotEqual(result.returncode, 0)
                self.assertFalse((self.root / 'target').exists())

    def test_missing_source_and_escaping_destination_are_rejected(self):
        for sample, destination in [('missing', 'output'), ('file-read', '../escape'),
                                     ('file-read', '/absolute'), ('bad/name', 'output')]:
            with self.subTest(sample=sample, destination=destination):
                self.assertEqual(self.run_builder(sample, destination).returncode, 2)
                self.assertFalse((self.root / 'target').exists())


if __name__ == '__main__':
    unittest.main(verbosity=2)
