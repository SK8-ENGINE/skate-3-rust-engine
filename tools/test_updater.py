"""Isolated tests: no UI, game process, real installation or network."""
import hashlib
import io
import json
from pathlib import Path
import tempfile
import threading
import unittest
from unittest.mock import patch
import zipfile
import updater as u


def metadata(build=2):
    return dict(schema=1, repository=u.REPO, target='windows-x64', build=build,
                tag=f'v{build}', revision='a' * 40)


class UpdaterTests(unittest.TestCase):
    def test_channels_and_identity(self):
        release = dict(draft=False, published_at='2026-01-01', prerelease=True)
        self.assertFalse(u.eligible(release, 'Stable'))
        self.assertTrue(u.eligible(release, 'Latest'))
        release['draft'] = True
        self.assertFalse(u.eligible(release, 'Latest'))
        with self.assertRaises(ValueError):
            u.identity({**metadata(), 'target': 'linux'})

    def test_pagination_order_and_channel(self):
        def release(build, prerelease=False):
            return dict(id=build, tag_name=f'v{build}', draft=False, prerelease=prerelease,
                        published_at='date', assets=[dict(name=n, state='uploaded', browser_download_url=f'{build}/{n}')
                        for n in (u.PACKAGE, u.PACKAGE+'.sha256', 'release.json')])
        def fetch(url, *args):
            if url.endswith('&page=1'):
                return json.dumps([release(4, True)] + [dict(draft=True)]*99).encode()
            if url.endswith('&page=2'):
                return json.dumps([release(3), release(2)]).encode()
            return json.dumps(metadata(int(url.split('/')[0]))).encode()
        with patch.object(u, 'fetch', fetch):
            self.assertEqual(u.discover(metadata(1), 'Stable', threading.Event())[0], 3)
            self.assertEqual(u.discover(metadata(1), 'Latest', threading.Event())[0], 4)
            self.assertIsNone(u.discover(metadata(5), 'Latest', threading.Event()))

    def package(self, extra=None):
        meta = metadata()
        meta['files'] = {n: hashlib.sha256(b'new').hexdigest() for n in u.FILES[:-1]}
        data = io.BytesIO()
        with zipfile.ZipFile(data, 'w') as z:
            for name in u.FILES:
                z.writestr(u.PREFIX+name, json.dumps(meta) if name == 'release.json' else b'new')
            if extra:
                z.writestr(extra, b'evil')
        archive = data.getvalue()
        assets = {n: dict(browser_download_url=n) for n in (u.PACKAGE, u.PACKAGE+'.sha256')}
        candidate = (2, 2, {}, assets, meta)
        def fetch(url, *args):
            return archive if url == u.PACKAGE else (hashlib.sha256(archive).hexdigest()+'  '+u.PACKAGE).encode()
        return candidate, fetch

    def test_safe_staging_and_traversal(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp)
            candidate, fetch = self.package()
            with patch.object(u, 'fetch', fetch):
                u.stage(candidate, root, threading.Event(), lambda _: None)
            self.assertEqual((root/'new/skate3rust.exe').read_bytes(), b'new')
            for malicious in (u.PREFIX+'../evil', u.PREFIX+'skate3rust.exe', u.PREFIX+'C:evil'):
                candidate, fetch = self.package(malicious)
                with patch.object(u, 'fetch', fetch), self.assertRaises(ValueError):
                    u.stage(candidate, root, threading.Event(), lambda _: None)

    def test_checksum_failure_and_cancel(self):
        candidate, fetch = self.package()
        def corrupt(url, *args):
            return b'bad' if url == u.PACKAGE else fetch(url, *args)
        with tempfile.TemporaryDirectory() as temp:
            with patch.object(u, 'fetch', corrupt), self.assertRaises(ValueError):
                u.stage(candidate, Path(temp), threading.Event(), lambda _: None)
            cancelled = threading.Event(); cancelled.set()
            with patch.object(u, 'fetch', fetch), self.assertRaises(InterruptedError):
                u.stage(candidate, Path(temp), cancelled, lambda _: None)

    def test_replacement_rollback_preserves_data(self):
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp); tx = root/'.update-transaction'
            for name in u.FILES:
                (root/name).parent.mkdir(parents=True, exist_ok=True)
                (root/name).write_bytes(b'old')
                (tx/'new'/name).parent.mkdir(parents=True, exist_ok=True)
                (tx/'new'/name).write_bytes(b'new')
            (root/'player-data').write_bytes(b'untouched')
            replace = u.os.replace
            def fail(src, dest):
                if str(src).endswith('new\\support\\skate3setup.exe') or str(src).endswith('new/support/skate3setup.exe'):
                    raise OSError('simulated locked file')
                return replace(src, dest)
            with patch.object(u.os, 'replace', fail), patch.object(u, 'retry', lambda op: op()), self.assertRaises(OSError):
                u.install(root, tx)
            for name in u.FILES:
                self.assertEqual((root/name).read_bytes(), b'old')
            self.assertEqual((root/'player-data').read_bytes(), b'untouched')
            self.assertFalse((tx/'journal.json').exists())


if __name__ == '__main__':
    unittest.main()
