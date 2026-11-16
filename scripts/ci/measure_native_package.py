#!/usr/bin/env python3
"""Measure native bytes and an actual npm tarball using identical package metadata.

All normalization is applied to generated copies. Inputs and installed packages
are never modified. --stripped-native reuses a previously stripped artifact.
"""
import argparse
import gzip
import hashlib
import json
import pathlib
import shutil
import subprocess
import sys
import tarfile


def digest(path):
    result = hashlib.sha256()
    with path.open('rb') as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b''):
            result.update(block)
    return result.hexdigest()


def run(command, **kwargs):
    return subprocess.run(command, check=True, text=True, capture_output=True, **kwargs)


def signature(path):
    result = subprocess.run(
        ['codesign', '--verify', '--verbose=2', str(path)],
        text=True, capture_output=True,
    )
    return {'valid': result.returncode == 0, 'details': (result.stdout + result.stderr).strip()}


def main():
    base = pathlib.Path(__file__).resolve().parent
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--variant', required=True)
    parser.add_argument('--native', required=True, type=pathlib.Path)
    parser.add_argument('--stripped-native', type=pathlib.Path)
    parser.add_argument('--already-stripped', action='store_true')
    parser.add_argument('--metadata', type=pathlib.Path,
                        default=base / 'toolchain/node_modules/@utoo/pack-darwin-arm64')
    parser.add_argument('--output-root', type=pathlib.Path,
                        default=base / 'native-package-sizes')
    args = parser.parse_args()
    if not args.variant or pathlib.Path(args.variant).name != args.variant:
        parser.error('--variant must be one directory name')
    source = args.native.resolve(strict=True)
    stripped_source = args.stripped_native.resolve(strict=True) if args.stripped_native else source
    directory = args.output_root.resolve() / args.variant
    stage = directory / 'package'
    stage.mkdir(parents=True, exist_ok=True)
    manifest_path = args.metadata / 'package.json'
    manifest = json.loads(manifest_path.read_text())
    native_name = manifest['main']
    if pathlib.Path(native_name).name != native_name:
        raise ValueError('Platform package main must be one file name')
    packaged_native = stage / native_name
    shutil.copyfile(manifest_path, stage / 'package.json')
    shutil.copyfile(args.metadata / 'README.md', stage / 'README.md')
    shutil.copyfile(stripped_source, packaged_native)
    for file in stage.iterdir():
        file.chmod(0o644)
    strip_command = None
    if not args.stripped_native and not args.already_stripped:
        strip_command = ['xcrun', 'strip', '-S', '-x', str(packaged_native)]
        run(strip_command)
    gzip_path = directory / (native_name + '.gz')
    with packaged_native.open('rb') as raw, gzip_path.open('wb') as output:
        # Empty filename and mtime=0 make the gzip header independent of paths/time.
        with gzip.GzipFile(filename='', mode='wb', fileobj=output,
                           compresslevel=6, mtime=0) as compressed:
            shutil.copyfileobj(raw, compressed, length=1024 * 1024)
    packed = json.loads(run(
        ['npm', 'pack', '--ignore-scripts', '--json', '--pack-destination', str(directory)],
        cwd=stage,
    ).stdout)[0]
    archive = directory / packed['filename']
    with tarfile.open(archive, 'r:gz') as tar:
        members = sorted(member.name for member in tar.getmembers() if member.isfile())
        archived_native = tar.extractfile('package/' + native_name)
        archive_hash = hashlib.sha256()
        for block in iter(lambda: archived_native.read(1024 * 1024), b''):
            archive_hash.update(block)
    native_hash = digest(packaged_native)
    assert archive_hash.hexdigest() == native_hash, 'npm tarball changed the native file'
    assert members == sorted(['package/README.md', 'package/package.json', 'package/' + native_name]), members
    report = {
        'variant': args.variant,
        'inputNative': str(source),
        'inputNativeBytes': source.stat().st_size,
        'inputNativeSha256': digest(source),
        'strippedInput': str(stripped_source),
        'stripCommand': strip_command,
        'packagedNative': str(packaged_native),
        'packagedNativeBytes': packaged_native.stat().st_size,
        'packagedNativeSha256': native_hash,
        'gzipLevel': 6,
        'gzipMtime': 0,
        'gzipFilename': '',
        'gzipBytes': gzip_path.stat().st_size,
        'gzipPath': str(gzip_path),
        'gzipSha256': digest(gzip_path),
        'npmVersion': run(['npm', '--version']).stdout.strip(),
        'npmTarball': str(archive),
        'npmTarballBytes': archive.stat().st_size,
        'npmTarballSha256': digest(archive),
        'npmUnpackedBytes': packed['unpackedSize'],
        'npmPackageFiles': packed['files'],
        'metadataManifestSha256': digest(manifest_path),
        'metadataReadmeSha256': digest(args.metadata / 'README.md'),
        'nativeMode': '0644',
        'signature': signature(packaged_native),
        'python': sys.version.split()[0],
    }
    report_path = directory / 'size.json'
    report_path.write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps({key: report[key] for key in [
        'variant', 'inputNativeBytes', 'packagedNativeBytes', 'gzipBytes',
        'npmTarballBytes', 'npmTarball', 'signature',
    ]}, indent=2))


if __name__ == '__main__':
    main()
