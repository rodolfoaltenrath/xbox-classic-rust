"""Launcher com Cargo/guest/PowerShell controlados; testes reais Win32 são separados."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

import test_sample_tools as fixtures

SCRIPTS = Path(__file__).resolve().parents[1]
POWERSHELL = r'''#!/usr/bin/env python3
import json, pathlib, subprocess, sys
a=sys.argv[1:]
folder=pathlib.Path(a[a.index('-RunDirectory')+1])
(folder/'powershell-args.json').write_text(json.dumps(a))
run=subprocess.run([a[a.index('-Executable')+1]],capture_output=True)
(folder/'stdout.log').write_bytes(run.stdout)
(folder/'stderr.log').write_bytes(run.stderr)
sys.exit(run.returncode)
'''


class AlphaLauncherTest(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='obsidian alpha ')
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        binary = self.root / 'bin'
        binary.mkdir()
        for name, body in {
            'cargo': fixtures.CARGO.replace('exe.write_text(GUEST)', f'exe.write_text({fixtures.GUEST!r})'),
            'powershell.exe': POWERSHELL,
            'rustc': '#!/bin/sh\necho rustc-fixture\n',
            'uname': '#!/bin/sh\necho "${TEST_OS:-Linux}"\n',
            'cygpath': '#!/bin/sh\nprintf "%s\\n" "$2"\n',
        }.items():
            file = binary / name
            file.write_text(body)
            file.chmod(0o755)
        self.xbe = self.root / 'sample with spaces.xbe'
        self.xbe.write_bytes(b'alpha-launcher-fixture')
        self.state = self.root / 'state with spaces'
        self.env = dict(os.environ, PATH=str(binary) + os.pathsep + os.environ['PATH'],
                        ALPHA_STATE_DIR=str(self.state), ALPHA_BUILD_DIR=str(self.root / 'build with spaces'))

    def run_alpha(self, *extra, headless=True, **env):
        args = ['hello', '--xbe', str(self.xbe)]
        if headless:
            args.append('--sem-janela')
        return subprocess.run(['bash', str(SCRIPTS / 'run-alpha.sh'), *args, *extra],
                              env=dict(self.env, **env), capture_output=True, text=True, timeout=20)

    def runs(self):
        return sorted((self.state / 'runs').iterdir(), key=lambda p: p.stat().st_mtime_ns)

    def test_cache_reuses_only_identical_input_and_keeps_preparation_and_logs(self):
        for _ in range(2):
            result = self.run_alpha()
            self.assertEqual(result.returncode, 1, result.stderr)
            self.assertIn('Orçamento alcançado', result.stdout)
        first, second = self.runs()
        self.assertEqual((first / 'project.txt').read_text(), (second / 'project.txt').read_text())
        self.assertNotEqual((first / 'prepared-project.txt').read_text(), (second / 'prepared-project.txt').read_text())
        self.assertTrue((first / 'build.log').is_file())
        self.assertTrue((first / 'stderr.log').is_file())

    def test_changed_cached_source_is_preserved_and_never_executed_as_current(self):
        self.assertEqual(self.run_alpha().returncode, 1)
        old = Path((self.runs()[0] / 'project.txt').read_text().strip())
        (old / 'src/main.rs').write_text('user edit')
        result = self.run_alpha()
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn('Cache inválido preservado', result.stderr)
        self.assertNotEqual(Path((self.runs()[-1] / 'project.txt').read_text().strip()), old)
        self.assertEqual((old / 'src/main.rs').read_text(), 'user edit')
        repaired = (self.runs()[-1] / 'project.txt').read_text()
        self.assertTrue(list((self.state / 'cache').glob('*.invalid-*')))
        self.assertEqual(self.run_alpha().returncode, 1)
        self.assertEqual((self.runs()[-1] / 'project.txt').read_text(), repaired)

    def test_explicit_project_must_match_new_translation_and_lock(self):
        self.assertEqual(self.run_alpha().returncode, 1)
        project = Path((self.runs()[0] / 'project.txt').read_text().strip())
        self.assertEqual(self.run_alpha('--projeto', str(project)).returncode, 1)
        (project / 'Cargo.lock').write_text('different lock')
        result = self.run_alpha('--projeto', str(project))
        self.assertEqual(result.returncode, 2)
        self.assertIn('difere da tradução/lock', result.stderr)

    def test_errors_in_preparation_build_or_guest_keep_the_real_status(self):
        for mode, status in [('translate-fail', 21), ('lock-fail', 24), ('build-fail', 22), ('runtime-fail', 23)]:
            with self.subTest(mode=mode):
                result = self.run_alpha(TEST_MODE=mode)
                self.assertEqual(result.returncode, status, result.stderr)
                self.assertNotIn('Execução encerrada (status 0)', result.stdout)

    def test_windows_backend_gets_intact_paths_and_window_options(self):
        result = self.run_alpha(headless=False, TEST_OS='MINGW64_NT', TEST_EXE='1')
        self.assertEqual(result.returncode, 1, result.stderr)
        args = json.loads((self.runs()[-1] / 'powershell-args.json').read_text())
        self.assertNotIn('-Headless', args)
        self.assertEqual(args[args.index('-RunDirectory') + 1], str(self.runs()[-1]))
        self.assertIn('build with spaces', args[args.index('-Executable') + 1])

    def test_invalid_options_missing_input_and_unavailable_window_stop_before_build(self):
        for args in [('--fatias', '0'), ('--fatias', '4294967296'), ('--fatias',),
                     ('--unknown',), ('--fatias', '1', '--fatias', '2'), ('--projeto', str(self.root / 'missing'))]:
            self.assertEqual(self.run_alpha(*args).returncode, 2)
        self.assertEqual(self.run_alpha(headless=False).returncode, 2)
        self.xbe.unlink()
        self.assertEqual(self.run_alpha().returncode, 2)
        self.assertFalse(self.state.exists())


if __name__ == '__main__':
    unittest.main(verbosity=2)
