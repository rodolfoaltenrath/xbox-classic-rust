"""Protege o piso de testes de cada SO usando Cargo controlado em uma cópia do gate."""
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

SOURCE = Path(__file__).resolve().parents[1]


class GateTest(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='obsidian gate ')
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        scripts = self.root / 'scripts'
        scripts.mkdir()
        shutil.copyfile(SOURCE / 'gate.sh', scripts / 'gate.sh')
        for name, count in [('gate-baseline.txt', 580), ('gate-baseline-windows.txt', 580), ('gate-baseline-linux.txt', 576)]:
            (scripts / name).write_text(str(count) + '\n')
        (self.root / 'rust-toolchain.toml').write_text('channel = "fixture"\n')
        binary = self.root / 'bin'
        binary.mkdir()
        for name, body in {
            'uname': 'echo "$TEST_OS"',
            'rustc': 'echo "rustc fixture"',
            'cargo': 'if [ "$1" = test ]; then echo "test result: ok. $TEST_COUNT passed; 0 failed;"; fi',
        }.items():
            file = binary / name
            file.write_text('#!/bin/sh\n' + body + '\n')
            file.chmod(0o755)
        self.env = dict(os.environ, PATH=str(binary) + os.pathsep + os.environ['PATH'])

    def run_gate(self, platform='MINGW64_NT', count=580):
        return subprocess.run(['bash', str(self.root / 'scripts/gate.sh')],
                              env=dict(self.env, TEST_OS=platform, TEST_COUNT=str(count)),
                              capture_output=True, text=True, timeout=10)

    def test_known_platforms_preserve_their_own_floor(self):
        for platform, count in [('MINGW64_NT', 580), ('Linux', 576)]:
            result = self.run_gate(platform, count)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            self.assertTrue(result.stdout.splitlines()[-1].startswith('GATE OK:'))

    def test_count_below_known_floor_fails(self):
        for platform, count in [('MINGW64_NT', 579), ('Linux', 575)]:
            result = self.run_gate(platform, count)
            self.assertEqual(result.returncode, 1)
            self.assertIn('abaixo do baseline', result.stdout)

    def test_new_count_updates_only_active_platform(self):
        self.assertEqual(self.run_gate(count=581).returncode, 0)
        self.assertEqual((self.root / 'scripts/gate-baseline-windows.txt').read_text(), '581\n')
        self.assertEqual((self.root / 'scripts/gate-baseline-linux.txt').read_text(), '576\n')
        self.assertEqual((self.root / 'scripts/gate-baseline.txt').read_text(), '580\n')

    def test_missing_or_invalid_baseline_is_not_zero(self):
        baseline = self.root / 'scripts/gate-baseline-windows.txt'
        baseline.unlink()
        self.assertEqual(self.run_gate().returncode, 1)
        baseline.write_text('invalid\n')
        self.assertEqual(self.run_gate().returncode, 1)


if __name__ == '__main__':
    unittest.main(verbosity=2)
