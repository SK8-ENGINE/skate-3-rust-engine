"""Publish one rolling prerelease for a feature branch."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import shutil
import subprocess
import zipfile

from updater import REPO, PACKAGE, PREFIX, identity, program_metadata

BRANCH_RE = re.compile(r'[A-Za-z0-9._/-]{1,120}')


def gh(*args, payload=None):
    command = ['gh', *args]
    if payload is not None:
        command += ['--input', '-']
    result = subprocess.run(command, input=json.dumps(payload) if payload is not None else None,
                            text=True, capture_output=True)
    if result.returncode:
        raise RuntimeError(result.stderr.strip() or 'GitHub command failed')
    return result.stdout


def api(path, method='GET', payload=None):
    return json.loads(gh('api', f'repos/{REPO}/{path}', '--method', method, payload=payload) or 'null')


def validate(directory):
    meta = json.loads((directory / 'release.json').read_text(encoding='utf-8-sig'))
    identity(meta)
    program_metadata(meta)
    archive = directory / PACKAGE
    with archive.open('rb') as stream:
        digest = hashlib.file_digest(stream, 'sha256').hexdigest()
    checksum = (directory / (PACKAGE + '.sha256')).read_text().split()
    if checksum != [digest, PACKAGE]:
        raise ValueError('Package checksum mismatch')
    with zipfile.ZipFile(archive) as z:
        if json.loads(z.read(PREFIX + 'release.json').decode('utf-8-sig')) != meta:
            raise ValueError('Package identity mismatch')
        for name, expected in meta['files'].items():
            if hashlib.sha256(z.read(PREFIX + name)).hexdigest() != expected:
                raise ValueError('Program checksum mismatch')
    return meta, digest


def publish(directory, branch):
    branch = (branch or '').strip()
    if not BRANCH_RE.fullmatch(branch):
        raise ValueError('Invalid branch name')
    meta, digest = validate(directory)
    releases = json.loads(gh('api', f'repos/{REPO}/releases?per_page=100', '--paginate', '--slurp'))
    release = next((r for page in releases for r in page if r['tag_name'] == branch), None)
    if release and (release.get('immutable') or not release.get('prerelease')):
        raise ValueError(f'Release tag {branch} must be a mutable prerelease')
    if release is None:
        release = api('releases', 'POST', dict(
            tag_name=branch,
            target_commitish=meta['revision'],
            name=branch,
            draft=True,
            prerelease=True,
            make_latest='false',
        ))
    (directory / (PACKAGE + '.sha256')).write_text(digest + '  ' + PACKAGE + '\n', encoding='ascii')
    for name in (PACKAGE, PACKAGE + '.sha256', 'release.json'):
        gh('release', 'upload', branch, str(directory / name), '--repo', REPO, '--clobber')
    current = api(f"releases/{release['id']}")
    assets = {a['name'] for a in current.get('assets', []) if a.get('state') == 'uploaded'}
    if not {PACKAGE, PACKAGE + '.sha256', 'release.json'} <= assets:
        raise ValueError('Uploaded branch release is incomplete')
    changes = gh('api', f'repos/{REPO}/commits?sha={meta["revision"]}&per_page=10')
    commits = json.loads(changes)
    notes = (f"Rolling build **{meta['build']}**, commit `{meta['revision'][:12]}`.\n\n"
             f"[Download Windows ZIP](https://github.com/{REPO}/releases/download/{branch}/{PACKAGE})\n\n"
             f"Choose **Branch** in the in-game Updates window and enter `{branch}`.\n\n"
             "### Recent commits\n\n" + '\n'.join(
                 f"- {c['sha'][:8]} {c['commit']['message'].splitlines()[0]}" for c in commits))
    api(f"releases/{release['id']}", 'PATCH', dict(
        name=branch,
        body=notes,
        target_commitish=meta['revision'],
        draft=False,
        prerelease=True,
        make_latest='false',
    ))
    api('git/refs/tags/' + branch, 'PATCH', dict(sha=meta['revision'], force=True))
    print(f'Published branch release {branch} build {meta["build"]}')


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('--directory', type=Path, required=True)
    parser.add_argument('--branch', required=True)
    publish(parser.parse_args().directory, parser.parse_args().branch)
