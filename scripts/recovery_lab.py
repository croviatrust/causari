#!/usr/bin/env python3
"""Reproducible, synthetic recovery stress test. No model API, no real project edits."""
import argparse
import hashlib
import json
from pathlib import Path
import shlex
import subprocess
import sys
import tempfile
import time


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', default='target/debug/re')
    parser.add_argument('--events', type=int, default=64)
    parser.add_argument('--files', type=int, default=128)
    args = parser.parse_args()
    if not 4 <= args.events <= 1000 or not 1 <= args.files <= 10000:
        parser.error('events must be 4..1000; files must be 1..10000')
    binary = str(Path(args.binary).resolve())
    started = time.perf_counter()
    with tempfile.TemporaryDirectory(prefix='causari-recovery-lab-') as directory:
        root = Path(directory)
        def run(*arguments, expect=0):
            result = subprocess.run([binary, *arguments], cwd=root, capture_output=True, text=True, timeout=120)
            if result.returncode != expect:
                raise RuntimeError(result.stderr + result.stdout)
            return result
        def digest():
            return {str(p.relative_to(root)): hashlib.sha256(p.read_bytes()).hexdigest()
                    for p in root.rglob('*') if p.is_file() and '.causari' not in p.relative_to(root).parts}
        run('init')
        for number in range(args.files):
            (root / f'file-{number:04}.bin').write_bytes(bytes(range(256)) * 4)
        culprit_index = args.events // 2
        ids = []
        for event in range(args.events):
            (root / 'state').write_text('good' if event < culprit_index else 'bad')
            (root / 'iteration').write_text(str(event))
            run('record', '-m', f'synthetic event {event}')
            ids.append((root / '.causari/refs/sessions/main').read_text().strip())
        (root / 'state').write_text('unrecorded user edit')
        (root / 'notes.txt').write_text('unrecorded user file')
        before = digest()
        run('revert', ids[culprit_index], '--dry-run')
        assert digest() == before, 'dry run modified the workspace'
        check = "from pathlib import Path; import sys; sys.exit(0 if Path('state').read_text() == 'good' else 1)"
        command = subprocess.list2cmdline([sys.executable, '-c', check]) if sys.platform == 'win32' else shlex.join([sys.executable, '-c', check])
        result = run('bisect', '--good', ids[0], '--bad', ids[-1], '--test', command)
        assert f'first bad event: {ids[culprit_index]}' in result.stdout, 'wrong culprit'
        assert digest() == before, 'bisect lost or modified user files'
        assert (root / '.causari/refs/sessions/main').read_text().strip() == ids[-1], 'HEAD moved'
        print(json.dumps({'scenario': 'synthetic; no AI model involved', 'events': args.events,
                          'binary_fixture_files': args.files, 'culprit_index': culprit_index,
                          'culprit_found': True, 'dry_run_preserved_workspace': True,
                          'bisect_preserved_workspace': True, 'head_unchanged': True,
                          'workspace_files_verified': len(before),
                          'elapsed_seconds': round(time.perf_counter() - started, 3)}, indent=2))

if __name__ == '__main__':
    main()
