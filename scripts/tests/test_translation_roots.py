"""Testa a orquestração Bash com Cargo/guest controlados, sem compilar Rust.

Executar no Linux/WSL: python3 scripts/tests/test_translation_roots.py
"""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

SCRIPTS = Path(__file__).resolve().parents[1]
FAKE_CARGO = r'''#!/usr/bin/env python3
import os, pathlib, sys
a = sys.argv[1:]
mode = os.environ['TEST_MODE']
if a[0] == 'run':
    if mode == 'translate-fail': sys.exit(21)
    out = pathlib.Path(a[a.index('--out') + 1])
    out.mkdir()
    (out / 'Cargo.toml').write_text('fixture')
    (out / 'args.txt').write_text('\n'.join(a))
else:
    if mode == 'build-fail': sys.exit(22)
    out = pathlib.Path(a[a.index('--target-dir') + 1]) / 'release'
    out.mkdir(parents=True, exist_ok=True)
    name = 'obsidian-native.exe' if os.environ.get('TEST_EXE') else 'obsidian-native'
    binary = out / name
    binary.write_text(GUEST)
    binary.chmod(0o755)
'''
GUEST = r'''#!/usr/bin/env python3
import os, pathlib, sys, time
p = pathlib.Path(os.environ['TEST_COUNTER'])
n = int(p.read_text()) + 1 if p.exists() else 1
p.write_text(str(n))
mode = os.environ['TEST_MODE']
if mode == 'timeout': time.sleep(10)
if mode == 'runtime-fail':
    print('falta em virt:0x00000040', file=sys.stderr); sys.exit(23)
if mode in ('discover', 'block', 'duplicate') and (n == 1 or mode == 'duplicate'):
    message = ('endereço 0xAbCd não tem bloco recompilado' if mode == 'block'
               else 'transferência em 0x10000: alvo 0x0000aBcD não foi recompilado')
    print(message, file=sys.stderr); sys.exit(1)
print('guest returned')
'''


class ScriptsTest(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="obsidian-script-tests-")
        self.addCleanup(self.temp.cleanup)
        self.base = Path(self.temp.name)
        self.bin = self.base / "bin"
        self.bin.mkdir()
        cargo = self.bin / "cargo"
        cargo.write_text(FAKE_CARGO.replace("binary.write_text(GUEST)", f"binary.write_text({GUEST!r})"))
        cargo.chmod(0o755)
        self.xbe = self.base / "image with spaces.xbe"
        self.xbe.write_bytes(b"fixture")

    def run_script(self, mode="success", roots=(), name="output with spaces", **overrides):
        env = dict(os.environ, PATH=str(self.bin) + os.pathsep + os.environ["PATH"],
                   TEST_MODE=mode, TEST_COUNTER=str(self.base / (name + ".count")),
                   ROOT_SEARCH_MAX_ROUNDS="3", ROOT_SEARCH_TIMEOUT_SECONDS="1")
        env.update(overrides)
        return subprocess.run(["bash", str(SCRIPTS / "find-translation-roots.sh"),
                               str(self.xbe), str(self.base / name), *roots],
                              env=env, capture_output=True, text=True, timeout=15)

    def test_discovers_both_diagnostics_and_keeps_previous_attempt(self):
        for mode in ("discover", "block"):
            with self.subTest(mode=mode):
                result = self.run_script(mode, name=mode)
                self.assertEqual(result.returncode, 0, result.stderr)
                output = self.base / mode
                self.assertEqual((output / "roots.txt").read_text().strip(), "0x0000ABCD")
                self.assertTrue((output / "attempt-01" / "Cargo.toml").is_file())
                self.assertIn("0x0000ABCD", (output / "attempt-02" / "args.txt").read_text())
                self.assertTrue((output / "attempt-01.run.err").is_file())
                self.assertEqual((output / "final-project.txt").read_text().strip(), str(output / "attempt-02"))

    def test_existing_directory_is_preserved(self):
        output = self.base / "existing"
        output.mkdir()
        sentinel = output / "keep.txt"
        sentinel.write_text("keep")
        result = self.run_script(name="existing")
        self.assertEqual(result.returncode, 2)
        self.assertEqual(sentinel.read_text(), "keep")
        self.assertEqual(list(output.iterdir()), [sentinel])

    def test_errors_and_timeout_keep_exit_status(self):
        for mode, expected in (("translate-fail", 21), ("build-fail", 22),
                               ("runtime-fail", 23), ("timeout", 124)):
            with self.subTest(mode=mode):
                result = self.run_script(mode, name=mode)
                self.assertEqual(result.returncode, expected, result.stderr)

    def test_repeated_root_is_detected_after_hex_normalization(self):
        result = self.run_script("duplicate", roots=("0xabcD", "0X0000ABCD"))
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn("já informado", result.stderr)
        self.assertEqual((self.base / "output with spaces" / "roots.txt").read_text(), "0x0000ABCD\n")

    def test_windows_binary_name_is_supported(self):
        result = self.run_script(TEST_EXE="1")
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_iteration_limit_is_not_success(self):
        result = self.run_script("discover", ROOT_SEARCH_MAX_ROUNDS="1")
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn("ainda não traduzida", result.stderr)

    def test_invalid_input_is_refused_before_creating_output(self):
        for root in ("0x100000000", "0xXYZ", "1234"):
            with self.subTest(root=root):
                self.assertEqual(self.run_script(roots=(root,)).returncode, 2)
        self.assertEqual(self.run_script(ROOT_SEARCH_MAX_ROUNDS="0").returncode, 2)
        self.assertFalse((self.base / "output with spaces").exists())
        for sample, dest in (("../hello", "ok"), ("hello", "../outside"), ("hello", "/tmp/outside")):
            result = subprocess.run(["bash", str(SCRIPTS / "build-nxdk-sample.sh"), sample, dest],
                                    capture_output=True, text=True, timeout=5)
            self.assertEqual(result.returncode, 2, result.stderr)


if __name__ == "__main__":
    unittest.main(verbosity=2)
