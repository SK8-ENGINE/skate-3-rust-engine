"""Find and strip byte order marks and other invisible characters (BOM sanitisation skill).

Usage (run from the repo or worktree concerned):
  python bom_scrub.py scan [--fix] PATH...      files / folders (text files only, binaries skipped)
  python bom_scrub.py staged [--fix]            files staged in git (--fix re-stages them)
  python bom_scrub.py msg FILE                  strip a commit message / PR body draft in place before use
  python bom_scrub.py log [N]                   report commit subjects / bodies of the last N commits (default 20)
  python bom_scrub.py stdin                     read text on stdin, print it cleaned (pipe a draft through it)

Exit code 1 when something was found (and not fixed), so it can gate a commit or a publish step.
"""
from __future__ import annotations

import subprocess
import sys
from pathlib import Path

# U+FEFF (BOM / zero width no-break space), zero width space / joiners, word joiner, soft hyphen.
INVISIBLE = {chr(0xFEFF): 'BOM U+FEFF', chr(0x200B): 'ZWSP U+200B', chr(0x200C): 'ZWNJ U+200C',
             chr(0x200D): 'ZWJ U+200D', chr(0x2060): 'WJ U+2060', chr(0x00AD): 'SHY U+00AD'}
BOM_BYTES = (b'\xef\xbb\xbf', b'\xff\xfe', b'\xfe\xff')
SKIP_DIRS = {'.git', 'target', 'node_modules', '__pycache__', '.venv', 'assets', 'data'}
TEXT_SUFFIXES = {'.md', '.txt', '.rs', '.py', '.toml', '.json', '.lua', '.wgsl', '.ps1', '.psm1', '.bat', '.cmd', '.sh',
                 '.yml', '.yaml', '.cs', '.cpp', '.h', '.hpp', '.c', '.cfg', '.ini', '.log', '.html', '.css', '.js',
                 '.ts', '.csv', '.tsv', '.xml', '.patch', '.diff'}


def findings(text: str) -> list[str]:
    """Each invisible character with its line and column (1-based)."""
    out = []
    for n, line in enumerate(text.splitlines(), 1):
        for col, ch in enumerate(line, 1):
            if ch in INVISIBLE:
                out.append(f'{n}:{col} {INVISIBLE[ch]}')
    return out


def clean(text: str) -> str:
    return ''.join(ch for ch in text if ch not in INVISIBLE)


def read_text(path: Path) -> str | None:
    """The file as UTF-8 (a leading BOM kept as U+FEFF), or None for binaries / other encodings."""
    raw = path.read_bytes()
    if raw.startswith(b'\xff\xfe') or raw.startswith(b'\xfe\xff'):
        return raw.decode('utf-16')
    if b'\x00' in raw[:4096]:
        return None
    try:
        return raw.decode('utf-8')
    except UnicodeDecodeError:
        return None


def write_text(path: Path, text: str) -> None:
    """UTF-8 without BOM, line endings as they were (newline='' keeps CRLF / LF untouched)."""
    with open(path, 'w', encoding='utf-8', newline='') as f:
        f.write(text)


def walk(paths: list[str]):
    for p in map(Path, paths):
        if p.is_dir():
            for f in p.rglob('*'):
                if f.is_file() and f.suffix.lower() in TEXT_SUFFIXES and not SKIP_DIRS & set(f.parts):
                    yield f
        elif p.is_file():
            yield p


def scan(paths: list[str], fix: bool) -> int:
    bad = 0
    for f in walk(paths):
        text = read_text(f)
        if text is None:
            continue
        hits = findings(text)
        utf16 = f.read_bytes()[:2] in (b'\xff\xfe', b'\xfe\xff')
        if not hits and not utf16:
            continue
        label = (['UTF-16 file'] if utf16 else []) + hits[:5] + (['...'] if len(hits) > 5 else [])
        if fix:
            write_text(f, clean(text))
            print(f'FIXED {f}: {", ".join(label)}')
        else:
            bad += 1
            print(f'FOUND {f}: {", ".join(label)}')
    return bad


def git(*args: str) -> str:
    return subprocess.run(['git', *args], capture_output=True, text=True, encoding='utf-8', check=True).stdout


def main(argv: list[str]) -> int:
    if not argv:
        print(__doc__)
        return 2
    cmd, rest = argv[0], argv[1:]
    fix = '--fix' in rest
    rest = [a for a in rest if a != '--fix']
    if cmd == 'scan':
        return 1 if scan(rest or ['.'], fix) else 0
    if cmd == 'staged':
        files = [f for f in git('diff', '--cached', '--name-only', '--diff-filter=ACMR').splitlines() if f]
        bad = scan(files, fix)
        if fix and files:
            git('add', '--', *files)
        return 1 if bad else 0
    if cmd == 'msg':
        p = Path(rest[0])
        text = read_text(p) or ''
        hits = findings(text)
        write_text(p, clean(text))
        print(f'msg {p}: ' + (f'stripped {len(hits)} ({", ".join(hits[:5])})' if hits else 'clean'))
        return 0
    if cmd == 'log':
        n = rest[0] if rest else '20'
        bad = 0
        for entry in git('log', f'-{n}', '--format=%h%x1f%B%x1e').split('\x1e'):
            if '\x1f' not in entry:
                continue
            sha, body = entry.strip('\n').split('\x1f', 1)
            hits = findings(body)
            if hits:
                bad += 1
                print(f'FOUND commit {sha}: {", ".join(hits[:5])} | {clean(body).splitlines()[0][:80] if body.strip() else ""}')
        if bad:
            print('Pushed commits: do not rewrite them unless the user asks; fix the writer instead (see SKILL.md).')
        return 1 if bad else 0
    if cmd == 'stdin':
        sys.stdout.write(clean(sys.stdin.read()))
        return 0
    print(__doc__)
    return 2


if __name__ == '__main__':
    sys.exit(main(sys.argv[1:]))
