"""Orquestração dos samples, com Cargo/guest controlados. Executar com python3."""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

SCRIPTS = Path(__file__).resolve().parents[1]
GUEST = r'''#!/usr/bin/env python3
import os, sys
mode = os.environ.get('TEST_MODE', '')
if mode == 'runtime-fail': print('Error: falha real'); sys.exit(23)
if mode == 'false-budget': print('Error: outro erro'); sys.exit(1)
if mode != 'missing-metrics':
    print('Kernel: 42 chamadas atendidas, 0 recusadas; instruções=123, EIP=0x10000')
    print('Framebuffer guest: saída habilitada, 12 pixels coloridos em 4 linhas')
    print('GPU raster: chamadas=1 triângulos=1 fragmentos=12')
    print('  regiões de cor dos vértices: R=1 G=1 B=1')
message = 'execução interrompida após 320 blocos; 42 chamadas atendidas, 0 recusadas;'
print('Error: ' + ('"' + message + '"' if mode == 'quoted-budget' else message))
sys.exit(1)
'''
CARGO = r'''#!/usr/bin/env python3
import os, pathlib, shutil, sys
a = sys.argv[1:]
mode = os.environ.get('TEST_MODE', '')
if a[0] == '--version': print('cargo fixture'); sys.exit(0)
if a[0] == 'run':
    if mode == 'translate-fail': sys.exit(21)
    out = pathlib.Path(a[a.index('--out') + 1]); (out / 'src').mkdir(parents=True)
    shutil.copyfile(a[a.index('translate') + 1], out / 'src/image.xbe')
    for name in ['Cargo.toml', 'src/generated.rs', 'src/main.rs']: (out / name).write_text('fixture')
    (out / 'args.txt').write_text('\n'.join(a))
elif a[0] == 'build':
    if mode == 'build-fail': sys.exit(22)
    out = pathlib.Path(a[a.index('--target-dir') + 1]) / 'release'; out.mkdir(parents=True, exist_ok=True)
    name = 'obsidian-native.exe' if os.environ.get('TEST_EXE') else 'obsidian-native'
    exe = out / name; exe.write_text(GUEST); exe.chmod(0o755)
'''


class SampleToolsTest(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='obsidian samples ')
        self.addCleanup(self.temp.cleanup)
        self.base = Path(self.temp.name)
        binary = self.base / 'bin'
        binary.mkdir()
        cargo = binary / 'cargo'
        cargo.write_text(CARGO.replace('exe.write_text(GUEST)', f'exe.write_text({GUEST!r})'))
        cargo.chmod(0o755)
        rustc = binary / 'rustc'
        rustc.write_text('#!/bin/sh\necho rustc-fixture\n')
        rustc.chmod(0o755)
        self.xbe = self.base / 'sample with spaces.xbe'
        self.xbe.write_bytes(b'input-xbe')
        self.env = dict(os.environ, PATH=str(binary) + os.pathsep + os.environ['PATH'],
                        SONDA_XBE=str(self.xbe), SONDA_TARGET_DIR=str(self.base / 'build with spaces'))
        self.env.pop('SONDA_PROJETO', None)

    def run_tool(self, script='sonda.sh', args=('triangle', '320'), **env):
        return subprocess.run(['bash', str(SCRIPTS / script), *args],
                              env=dict(self.env, **env), capture_output=True, text=True, timeout=20)

    def test_preparation_preserves_new_projects_and_input_without_manual_entries(self):
        outputs = []
        for _ in range(2):
            result = self.run_tool('prepare-sample.sh', ('triangle', str(self.xbe)))
            self.assertEqual(result.returncode, 0, result.stderr)
            project = Path(result.stdout.strip())
            outputs.append(project)
            self.assertEqual((project / 'src/image.xbe').read_bytes(), b'input-xbe')
            self.assertTrue((project / 'input.sha256').is_file())
            self.assertNotIn('--entry', (project / 'args.txt').read_text())
        self.assertNotEqual(*outputs)
        self.assertTrue(all(p.exists() for p in outputs))

    def test_successful_budget_has_complete_summary_and_windows_binary_name(self):
        for mode in ['', 'quoted-budget']:
            result = self.run_tool(TEST_EXE='1', TEST_MODE=mode)
            self.assertEqual(result.returncode, 0, result.stderr)
            for text in ['== RESUMO', 'recusadas: 0', 'chamadas: 42', 'framebuffer:', 'raster:', 'mmio_sem_modelo: 0']:
                self.assertIn(text, result.stdout)

    def test_translation_build_and_runtime_errors_keep_status_without_summary(self):
        for mode, status in [('translate-fail', 21), ('build-fail', 22), ('runtime-fail', 23), ('false-budget', 1)]:
            with self.subTest(mode=mode):
                result = self.run_tool(TEST_MODE=mode)
                self.assertEqual(result.returncode, status, result.stderr)
                self.assertNotIn('== RESUMO', result.stdout)

    def test_missing_metrics_is_not_success(self):
        result = self.run_tool(TEST_MODE='missing-metrics')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('métricas obrigatórias ausentes', result.stderr)

    def test_invalid_inputs_and_project_fail_explicitly(self):
        for args in [('unknown',), ('triangle', '0'), ('triangle', '2', 'extra')]:
            self.assertEqual(self.run_tool(args=args).returncode, 2)
        self.assertEqual(self.run_tool(SONDA_XBE=str(self.base / 'absent.xbe')).returncode, 2)
        self.assertEqual(self.run_tool(SONDA_PROJETO=str(self.base)).returncode, 2)

    def test_explicit_project_must_match_requested_image(self):
        result = self.run_tool('prepare-sample.sh', ('hello', str(self.xbe)))
        self.assertEqual(result.returncode, 0, result.stderr)
        project = result.stdout.strip()
        self.assertEqual(self.run_tool(SONDA_PROJETO=project).returncode, 0)
        self.xbe.write_bytes(b'another-image')
        result = self.run_tool(SONDA_PROJETO=project)
        self.assertEqual(result.returncode, 2)
        self.assertIn('XBE diferente', result.stderr)


if __name__ == '__main__':
    unittest.main(verbosity=2)
