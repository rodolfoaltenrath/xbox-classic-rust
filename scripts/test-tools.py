#!/usr/bin/env python3
"""Run tooling tests with exact script copies in an isolated Git fixture.

Use Linux/WSL (bash, git and python3). An ext4 /tmp avoids repeated git diff
over the Windows mount, where the existing 20-second test limit can expire.
No test or timeout is changed; no real Cargo, SDK or guest executable runs.
"""
import hashlib
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile


def main():
    source = Path(__file__).resolve().parent
    with tempfile.TemporaryDirectory(prefix='obsidian-tools-') as temporary:
        root = Path(temporary)
        copied = root / 'scripts'
        shutil.copytree(source, copied, ignore=shutil.ignore_patterns('__pycache__'))
        digest = hashlib.sha256()
        for file in sorted(copied.rglob('*')):
            if file.is_file():
                relative = file.relative_to(copied)
                if file.read_bytes() != (source / relative).read_bytes():
                    raise RuntimeError(f'Copy differs from source: {relative}')
                digest.update(str(relative).encode())
                digest.update(hashlib.sha256(file.read_bytes()).digest())
        subprocess.run(['git', '-C', str(root), 'init', '-q'], check=True)
        subprocess.run(['git', '-C', str(root), '-c', 'core.autocrlf=false',
                        'add', 'scripts'], check=True)
        # Fixture identity is local to this disposable repository, never global.
        subprocess.run(['git', '-C', str(root), '-c', 'user.name=Test Fixture',
                        '-c', 'user.email=fixture@localhost', 'commit', '-qm',
                        'test fixture'], check=True)
        print(f'Tool scripts SHA-256: {digest.hexdigest()}', flush=True)
        result = subprocess.run([sys.executable, '-m', 'unittest',
                                 'test_alpha_launcher', 'test_sample_tools',
                                 'test_gate', 'test_guest_builder', '-v'], cwd=copied / 'tests')
        if result.returncode:
            return result.returncode
        print('TOOLS OK', flush=True)
        return 0


if __name__ == '__main__':
    sys.exit(main())
