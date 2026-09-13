"""Check exact RGBA preservation and the frozen Metal/rt64 residual for one registered row."""

import argparse
import gzip
import hashlib
import json
from pathlib import Path
import struct


def compare(scene, parent, candidate, reference):
    here = Path(__file__).resolve().parent
    rows = json.loads((here / 'oracle-bb0fea3.json').read_text())['rows']
    row = next((row for row in rows if row['scene'] == scene), None)
    if row is None:
        raise ValueError(f'unregistered oracle row: {scene}')
    a, b, r = [path.read_bytes() for path in (parent, candidate, reference)]
    expected = gzip.decompress((here / row['residual_file']).read_bytes())
    return compare_bytes(row, a, b, r, expected)


def compare_bytes(row, a, b, r, expected):
    scene = row['scene']
    if not len(a) == len(b) == len(r) == row['rgba_bytes']:
        raise ValueError('missing or truncated RGBA row')
    if hashlib.sha256(a).hexdigest() != row['fast3d_sha256']:
        raise ValueError('parent RGBA differs from the frozen C2 anchor')
    if a != b:
        raise ValueError(f'{scene}: parent/candidate RGBA differs at threshold 0')
    if hashlib.sha256(r).hexdigest() != row['rt64_sha256']:
        raise ValueError('reference readback differs from the pinned C2 input/output baseline')
    channels = len(row['channels'])
    residual = b''.join(struct.pack('<h', int(b[i]) - int(r[i]))
                        for i in range(len(b)) if i % 4 < channels)
    if residual != expected:
        raise ValueError(f'{scene}: changed residual values or mask; baseline review required')
    return {'scene': scene, 'parent_candidate_changed_bytes': 0, 'residual': 'exact frozen values'}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('scene')
    for name in ('parent', 'candidate', 'reference'):
        parser.add_argument(name, type=Path)
    args = parser.parse_args()
    try:
        print(json.dumps(compare(args.scene, args.parent, args.candidate, args.reference)))
    except (ValueError, OSError) as error:
        parser.exit(1, f'blocked: {error}\n')


if __name__ == '__main__':
    main()
