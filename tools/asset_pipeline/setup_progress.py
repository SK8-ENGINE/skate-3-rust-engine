"""Setup progress for another front end (the game's startup splash, design b172 / doc 31 "Asset refresh on the splash").

One JSON object per line, UTF-8, flushed per line, version 1:
  {"v":1,"kind":"start","refresh":true,"groups":["audio","maps"]}
  {"v":1,"kind":"step","text":"Converting 12 maps with 4 workers"}
  {"v":1,"kind":"item","text":"Converted 3/12 maps: The Park","done":3,"total":12}
  {"v":1,"kind":"needs_window","reason":"..."}
  {"v":1,"kind":"error","text":"...","log":"<base>/setup-error.log"}
  {"v":1,"kind":"done","warnings":2,"report":"<installation>/setup-report.json"}
Readers ignore unknown kinds and fields.
"""
import json
from pathlib import Path


class ProgressWriter:
    """A `report` callable (setup's progress text) that also writes the protocol; `.progress(done, total, text)` is the
    optional counted form install.py calls where it knows the fraction (plain lambdas lack it and lose nothing)."""

    def __init__(self, path, echo=None):
        self.path = Path(path)
        self.echo = echo
        self.path.parent.mkdir(parents=True, exist_ok=True)
        self.file = self.path.open('a', encoding='utf-8', newline='\n')

    def write(self, kind, **fields):
        self.file.write(json.dumps({'v': 1, 'kind': kind, **fields}, ensure_ascii=False) + '\n')
        self.file.flush()

    def __call__(self, text):
        self.write('step', text=text)
        if self.echo:
            self.echo(text)

    def progress(self, done, total, text):
        self.write('item', text=text, done=done, total=total)
        if self.echo:
            self.echo(text)

    def close(self):
        self.file.close()


def counted(report, done, total, text):
    """`report.progress(...)` when the reporter has it, else the plain text."""
    progress = getattr(report, 'progress', None)
    if progress:
        progress(done, total, text)
    else:
        report(text)
