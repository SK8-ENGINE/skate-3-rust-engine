"""Isolated tests: no UI, game process, real installation or network."""
import hashlib
import io
import json
import os
from pathlib import Path
import tempfile
import threading
import unittest
from unittest.mock import patch
import zipfile
import updater as u


def metadata(build=2):
    return dict(schema=1, repository=u.REPO, target='windows-x64', build=build,
                tag=f'v{build}', revision='a' * 40, files={n: 'b'*64 for n in u.FILES[:-1]})


class UpdaterTests(unittest.TestCase):
    def test_steam_relay_is_part_of_the_update_transaction(self):
        self.assertIn('steam-relay/skate-steam-relay.exe', u.FILES)
        self.assertIn('steam-relay/steam_api64.dll', u.FILES)
        # Metadata is replaced last, after all program components.
        self.assertEqual(u.FILES[-1], 'release.json')

    def test_channels_and_identity(self):
        release = dict(draft=False, published_at='2026-01-01', prerelease=True)
        self.assertFalse(u.eligible(release, 'Stable'))
        self.assertTrue(u.eligible(release, 'Latest'))
        release['draft'] = True
        self.assertFalse(u.eligible(release, 'Latest'))
        with self.assertRaises(ValueError):
            u.identity({**metadata(), 'target': 'linux'})
        self.assertEqual(u.identity({**metadata(0), 'tag': 'development'}), 0)
        with self.assertRaises(ValueError):
            u.identity({**metadata(0), 'tag': 'v0'})

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

    def test_rolling_release_same_tag_new_build_and_partial_upload(self):
        def release():
            names = []
            for build in (2, 3):
                names += [f'release-{build}.json', f'skate3rust-windows-x64-build-{build}.zip',
                          f'skate3rust-windows-x64-build-{build}.zip.sha256']
            names += ['release-4.json']  # Incomplete upload must not be offered.
            return dict(id=1, tag_name='experimental', draft=False, prerelease=True,
                        published_at='date', assets=[dict(name=n, state='uploaded', browser_download_url=n) for n in names])
        def fetch(url, *args):
            if url.startswith(u.API):
                return json.dumps([release()]).encode()
            build = int(url.removeprefix('release-').removesuffix('.json'))
            return json.dumps({**metadata(build), 'tag': 'experimental'}).encode()
        with patch.object(u, 'fetch', fetch):
            current = {**metadata(2), 'tag': 'experimental'}
            self.assertEqual(u.discover(current, 'Latest', threading.Event())[0], 3)
            self.assertIsNone(u.discover(current, 'Stable', threading.Event()))
            self.assertIsNone(u.discover({**current, 'build': 3}, 'Latest', threading.Event()))

    def package(self, extra=None, program=None):
        meta = metadata()
        meta['files'] = {n: hashlib.sha256(b'new').hexdigest() for n in u.FILES[:-1]}
        if program:
            meta['files'][program] = hashlib.sha256(b'new').hexdigest()
        data = io.BytesIO()
        with zipfile.ZipFile(data, 'w') as z:
            for name in [*meta['files'], 'release.json']:
                z.writestr(u.PREFIX+name, json.dumps(meta) if name == 'release.json' else b'new')
            if extra:
                z.writestr(extra, b'evil')
        archive = data.getvalue()
        assets = {n: dict(browser_download_url=n) for n in (u.PACKAGE, u.PACKAGE+'.sha256')}
        candidate = (2, 2, {}, assets, meta)
        def fetch(url, *args):
            return archive if url == u.PACKAGE else (hashlib.sha256(archive).hexdigest()+'  '+u.PACKAGE).encode()
        return candidate, fetch

    def test_new_program_component_is_staged_without_updater_changes(self):
        with tempfile.TemporaryDirectory() as temp:
            root=Path(temp)
            name='future-tools/importer/new.dll'
            candidate, fetch=self.package(program=name)
            with patch.object(u,'fetch',fetch):
                u.stage(candidate,root,threading.Event(),lambda _:None)
            self.assertEqual((root/'new'/name).read_bytes(),b'new')

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

    def test_same_build_repair_is_available_without_downgrade(self):
        candidate, fetch = self.package()
        meta = candidate[4]
        release = dict(id=1, tag_name=meta['tag'], draft=False,
                       published_at='date', prerelease=False,
                       assets=[dict(name=n, state='uploaded', browser_download_url=n)
                               for n in (u.PACKAGE, u.PACKAGE+'.sha256', 'release.json')])
        def api(url, *args):
            if url.startswith(u.API): return json.dumps([release]).encode()
            if url=='release.json': return json.dumps(meta).encode()
            return fetch(url,*args)
        with patch.object(u,'fetch',api):
            self.assertIsNone(u.discover(meta,'Stable',threading.Event()))
            self.assertEqual(u.discover(meta,'Stable',threading.Event(),repair=True)[0],meta['build'])
            self.assertIsNone(u.discover({**meta,'build':meta['build']+1},'Stable',threading.Event(),repair=True))

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

    def test_offline_and_rate_limit(self):
        with patch.object(u, 'fetch', side_effect=OSError('offline')), self.assertRaises(OSError):
            u.discover(metadata(1), 'Stable', threading.Event())
        error = u.urllib.error.HTTPError(u.API, 429, 'rate limit', {}, None)
        with patch.object(u.urllib.request.OpenerDirector, 'open', side_effect=error), self.assertRaisesRegex(ValueError, 'rate limit'):
            u.fetch(u.API, threading.Event(), 100)

    def test_redirect_never_forwards_private_token(self):
        req = u.urllib.request.Request(u.API + '/assets/1', headers={'Authorization': 'Bearer test-only'})
        redirected = u.DownloadRedirect().redirect_request(req, None, 302, 'Found', {}, 'https://release-assets.githubusercontent.com/file')
        self.assertIsNone(redirected.get_header('Authorization'))

    def test_github_api_token_on_actions_requests(self):
        with patch.dict(os.environ, {'SKATE_UPDATE_GITHUB_TOKEN': 'test-token'}, clear=False):
            headers = u.github_request_headers(u.BRANCHES_API)
            self.assertEqual(headers.get('Authorization'), 'Bearer test-token')
            headers = u.github_request_headers(f'https://github.com/{u.REPO}/releases/download/v1/{u.PACKAGE}')
            self.assertNotIn('Authorization', headers)

    def test_branch_release_discovery_and_stage(self):
        meta = {**metadata(11), 'tag': 'experimental', 'revision': 'c' * 40}
        meta['files'] = {name: hashlib.sha256(b'new').hexdigest() for name in u.FILES[:-1]}
        archive = io.BytesIO()
        with zipfile.ZipFile(archive, 'w') as z:
            for name in [*meta['files'], 'release.json']:
                z.writestr(u.PREFIX + name, json.dumps(meta) if name == 'release.json' else b'new')
        package_bytes = archive.getvalue()
        digest = hashlib.sha256(package_bytes).hexdigest()
        release = dict(
            id=88,
            tag_name='skyline-driving-update',
            draft=False,
            prerelease=True,
            assets=[
                dict(name=u.PACKAGE, state='uploaded', browser_download_url='package'),
                dict(name=u.PACKAGE + '.sha256', state='uploaded', browser_download_url='checksum'),
                dict(name='release.json', state='uploaded', browser_download_url='manifest'),
            ],
        )

        def fetch(url, *args):
            if url.endswith('/tags/skyline-driving-update'):
                return json.dumps(release).encode()
            if url == 'manifest':
                return json.dumps(meta).encode()
            if url == 'checksum':
                return (digest + '  ' + u.PACKAGE).encode()
            if url == 'package':
                return package_bytes
            raise AssertionError(url)

        current = metadata(10)
        with patch.object(u, 'fetch', fetch):
            candidate = u.discover_branch_release(current, 'skyline-driving-update', threading.Event())
            self.assertIsNotNone(candidate)
            self.assertEqual(candidate[0], 11)
            with tempfile.TemporaryDirectory() as temp:
                u.stage(candidate, Path(temp), threading.Event(), lambda _: None)
                self.assertEqual((Path(temp) / 'new/skate3rust.exe').read_bytes(), b'new')

    def test_branch_discovery_and_stage(self):
        meta = {**metadata(9), 'tag': 'experimental', 'revision': 'b' * 40}
        meta['files'] = {name: hashlib.sha256(b'new').hexdigest() for name in u.FILES[:-1]}
        package = u.package_name(meta)
        archive = io.BytesIO()
        with zipfile.ZipFile(archive, 'w') as inner:
            for name in [*meta['files'], 'release.json']:
                inner.writestr(u.PREFIX + name, json.dumps(meta) if name == 'release.json' else b'new')
        package_bytes = archive.getvalue()
        digest = hashlib.sha256(package_bytes).hexdigest()
        artifact = io.BytesIO()
        with zipfile.ZipFile(artifact, 'w') as outer:
            outer.writestr(package, package_bytes)
            outer.writestr(package + '.sha256', f'{digest}  {package}')
            outer.writestr('release.json', json.dumps(meta))
        artifact_bytes = artifact.getvalue()

        def fetch(url, *args):
            if '/releases/tags/' in url:
                raise u.urllib.error.HTTPError(url, 404, 'not found', {}, None)
            if url.startswith(u.BRANCHES_API):
                return json.dumps([{'name': 'main'}, {'name': 'skyline'}]).encode()
            if '/workflows/' in url and '/runs?' in url:
                return json.dumps({'workflow_runs': [
                    {'id': 77, 'conclusion': 'success', 'head_sha': meta['revision']},
                ]}).encode()
            if url.endswith('/artifacts'):
                return json.dumps({'artifacts': [
                    {'name': u.ARTIFACT_NAME, 'archive_download_url': 'https://api.github.com/repos/' + u.REPO + '/actions/artifacts/1/zip'},
                ]}).encode()
            if url.endswith('/artifacts/1/zip'):
                return artifact_bytes
            raise AssertionError(url)

        current = metadata(8)
        with patch.object(u, 'fetch', fetch):
            names = u.list_branches(threading.Event())
            self.assertIn('skyline', names)
            candidate = u.discover_branch(current, 'skyline', threading.Event())
            self.assertIsNotNone(candidate)
            self.assertEqual(candidate[0], 9)
            with tempfile.TemporaryDirectory() as temp:
                u.stage(candidate, Path(temp), threading.Event(), lambda _: None)
                self.assertEqual((Path(temp) / 'new/skate3rust.exe').read_bytes(), b'new')
            self.assertIsNone(u.discover_branch({**current, 'revision': meta['revision']}, 'skyline', threading.Event()))


if __name__ == '__main__':
    unittest.main()
