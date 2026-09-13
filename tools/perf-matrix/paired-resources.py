#!/usr/bin/env python3
"""Pure source-bound original068/candidate allocation and RTX measurements.
Usage: python3 tools/perf-matrix/paired-resources.py ARCHIVE OUTPUT [--assert-candidate]
Outputs exact adapted inputs and hashes. No capture/socket/crypto sources execute.
Logical cache bytes are distinct from allocator requested bytes; neither is RSS.
"""
import hashlib, json, pathlib, subprocess, sys, difflib
ROOT = pathlib.Path(__file__).resolve().parents[2]
archive, output = map(lambda p: pathlib.Path(p).resolve(), sys.argv[1:3])
output.mkdir(parents=True, exist_ok=False)
manifest = json.loads((archive / 'source-manifest.json').read_text())
assert manifest['sourceCommit'] == '068b6628df5dc57f264446f8ec51215b37c51b6f'
def sha(data): return hashlib.sha256(data).hexdigest()
for path, item in manifest['files'].items():
    assert sha((archive / path).read_bytes()) == item['sha256'], path

def run(args):
    record = {'command': args, 'exit': None, 'stdout': '', 'stderr': ''}
    try:
        result = subprocess.run(args, capture_output=True, text=True)
        record.update(exit=result.returncode, stdout=result.stdout, stderr=result.stderr)
    except OSError as error:
        record['launchError'] = str(error)
        raise
    finally:
        # Persist diagnostics before propagating a failed command, including
        # warnings emitted on stderr by an otherwise successful compiler.
        receipt['commands'].append(record)
        (output / 'receipt.json').write_text(json.dumps(receipt, indent=2) + '\n')
    if result.returncode:
        raise RuntimeError(f'{args}: {result.returncode}\n{result.stdout}\n{result.stderr}')
    return result.stdout

def declaration(source, start, end):
    return source[source.index(start):source.index(end)]

receipt = {'baseline': manifest, 'inputs': {}, 'results': {}, 'commands': [], 'adaptations': {}}
for label, root in [('original068', archive), ('candidate', ROOT)]:
    dst = output / label
    dst.mkdir()
    fec_path = 'native/android-viewer/src/media_datagram.rs'
    source = (root / fec_path).read_text()
    # Verbatim production structs and FecGroup implementation; omit unrelated decoder adapters.
    selected = ''.join([
        declaration(source, "pub struct FrameFragment<'a>", 'pub fn parse_parity'),
        declaration(source, 'pub struct RestoredFragment', '/// Recently completed FEC groups.'),
        declaration(source, 'pub struct FecGroup', '#[derive(Debug, Clone, PartialEq, Eq)]\npub struct ReassembledFrame')])
    core = (root / 'crates/fec-core/src/lib.rs').read_text()
    (dst / 'fec-core.rs').write_text(core)
    constant = declaration(source, 'pub const MAX_DATAGRAM_BYTES:', '\n\n/// Per-socket')
    generated = '#![allow(dead_code)]\n' + constant + '\n' + selected + (ROOT / 'tools/perf-matrix/fec-allocation.rs').read_text()
    (dst / 'fec.rs').write_text(generated)
    setup_path = 'native/macos-capture-shim/Sources/Capture/CaptureSession+Setup.swift'
    ring_path = 'native/macos-capture-shim/Sources/Transport/MediaRetransmitRing.swift'
    if label == 'original068':
        ring_source = (root / setup_path).read_text()
        ring = 'import Foundation\n' + declaration(ring_source, 'final class MediaRetransmitRing', '/// Send-result contract')
        ring_origin = setup_path
    else:
        ring_origin = ring_path if (root / ring_path).exists() else setup_path
        ring_source = (root / ring_origin).read_text()
        ring = ring_source if ring_origin == ring_path else 'import Foundation\n' + declaration(ring_source, 'final class MediaRetransmitRing', '/// Send-result contract')
    (dst / 'ring.swift').write_text(ring)
    geometry_path = 'native/macos-capture-shim/Sources/Split/SplitGeometry.swift'
    (dst / 'geometry.swift').write_bytes((root / geometry_path).read_bytes())
    receipt['inputs'][label] = {p: sha((root / p).read_bytes()) for p in [fec_path, 'crates/fec-core/src/lib.rs', ring_origin, geometry_path]}
    receipt['adaptations'][label] = {}
    for name, original, adapted in [('fec', source, generated), ('ring', ring_source, ring)]:
        patch = ''.join(difflib.unified_diff(original.splitlines(True), adapted.splitlines(True), fromfile='production', tofile='isolated'))
        (dst / f'{name}-extraction.diff').write_text(patch)
        receipt['adaptations'][label][name] = {'sha256': sha(adapted.encode()), 'diffSha256': sha(patch.encode())}
    commands = [
        ['rustc', '--edition=2021', '-O', '--crate-name', 'fec_core', '--crate-type=rlib', str(dst / 'fec-core.rs'), '-o', str(dst / 'libfec_core.rlib')],
        ['rustc', '--edition=2021', '-O', str(dst / 'fec.rs'), '--extern', f'fec_core={dst}/libfec_core.rlib', '-o', str(dst / 'fec')],
        ['/usr/bin/xcrun', 'swiftc', '-O', '-module-cache-path', str(output / 'swift-cache'), str(dst / 'ring.swift'), str(dst / 'geometry.swift'), str(ROOT / 'tools/perf-matrix/retransmit-benchmark.swift'), '-o', str(dst / 'ring')]]
    for command in commands: run(command)
# Both implementations compiled before either measurement, in the same invocation.
for label in ['original068', 'candidate']:
    receipt['results'][label] = {}
    for name in ['fec', 'ring']:
        command = [str(output / label / name)]
        if '--assert-candidate' in sys.argv and label == 'candidate': command.append('--assert-zero' if name == 'fec' else '--assert-bounded')
        lines = run(command)
        receipt['results'][label][name] = [json.loads(line) for line in lines.splitlines()]
for label, root in [('original068', archive), ('candidate', ROOT)]:
    for path, expected in receipt['inputs'][label].items():
        assert sha((root / path).read_bytes()) == expected, f'source changed during measurement: {path}'
receipt['fixtureHashes'] = {p: sha((ROOT / p).read_bytes()) for p in ['tools/perf-matrix/fec-allocation.rs', 'tools/perf-matrix/retransmit-benchmark.swift', 'tools/perf-matrix/paired-resources.py']}
(output / 'receipt.json').write_text(json.dumps(receipt, indent=2) + '\n')
print(json.dumps(receipt['results'], indent=2))
