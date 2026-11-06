import argparse
from fractions import Fraction
import hashlib
import json
import math
from pathlib import Path
import struct
import tempfile

import compact
import model


def digest(data):
    return hashlib.sha256(data).hexdigest()


def commands(data):
    assert data[:8] == b'RDPDUMP2'
    offset = 16
    while offset < len(data):
        tag, = struct.unpack_from('<I', data, offset)
        offset += 4
        if tag in (1, 8):
            _, length = struct.unpack_from('<II', data, offset)
            offset += 8 + length
        elif tag == 2:
            _, count = struct.unpack_from('<II', data, offset)
            offset += 8
            yield list(struct.unpack_from(f'<{count}I', data, offset))
            offset += count * 4
        else:
            assert tag in (5, 6, 7, 9), tag


def clip(poly, channel, boundary, above):
    result = []
    for a, b in zip(poly, poly[1:] + poly[:1]):
        va, vb = a[channel + 2] - boundary, b[channel + 2] - boundary
        ina, inb = (va >= 0, vb >= 0) if above else (va <= 0, vb <= 0)
        if ina:
            result.append(list(a))
        if ina != inb:
            t = va / (va - vb)
            result.append([x + (y - x) * t for x, y in zip(a, b)])
    return result


def byte_triangles(vertices):
    polys = [[list(map(float, (x, y, *[Fraction(c) for c in rgba]))) for x, y, rgba in vertices]]
    for channel in range(4):
        split = []
        for poly in polys:
            lo, hi = min(p[channel + 2] for p in poly), max(p[channel + 2] for p in poly)
            for cycle in range(math.floor(lo / 512), math.floor(hi / 512) + 1):
                for start, end, ramp, value in [(0, 255, True, 0), (255, 384, False, 255), (384, 512, False, 0)]:
                    left, right = start + cycle * 512, end + cycle * 512
                    if hi < left or lo > right:
                        continue
                    if lo == hi and not left <= lo < right:
                        continue
                    part = clip(clip(poly, channel, left, True), channel, right, False)
                    if len(part) < 3:
                        continue
                    for point in part:
                        point[channel + 2] = point[channel + 2] - cycle * 512 if ramp else value
                    split.append(part)
        polys = split
    result = []
    for poly in polys:
        for i in range(1, len(poly) - 1):
            triangle = [poly[0], poly[i], poly[i + 1]]
            area = sum(a[0] * b[1] - b[0] * a[1] for a, b in zip(triangle, triangle[1:] + triangle[:1]))
            if abs(area) > 1e-6:
                result.append([[round(p[0] * 128), round(p[1] * 128), *[max(0, min(255, round(c))) for c in p[2:]]] for p in triangle])
    return result


def edge_distance(vertices, x, y):
    distances = []
    for a, b in zip(vertices, vertices[1:] + vertices[:1]):
        ax, ay = a[:2]
        bx, by = b[:2]
        dx, dy = bx - ax, by - ay
        distances.append(abs(dx * (y - ay) - dy * (x - ax)) / max(1e-12, math.hypot(dx, dy)))
    return min(distances)


def generate(root, destination):
    destination.mkdir(parents=True, exist_ok=True)
    sources = [(p, 'matrix') for p in sorted((root / 'cases/matrix-final').glob('*.json'))]
    sources += [(p, 'modes') for p in sorted((root / 'results/modes/cases').glob('*.json'))]
    sources += [(p, 'interp') for p in sorted((root / 'results/interp/cases').glob('*.json'))]
    sources += [(root / f'results/gradients/cases/{rule}-forward.json', 'timing') for rule in ('shade-next', 'memory-previous', 'coverage-previous', 'compare-next')]
    geometry = json.loads((root / 'results/modes/geometry.json').read_text())['coverage']
    cases = []
    for path, group in sources:
        meta = json.loads(path.read_text())
        name, state = meta['id'], meta['rdp_state']
        run = 'rom/runs/matrix-hw-02-case05' if name.startswith('fog-05-') else 'rom/runs/matrix-hw-01' if group == 'matrix' else 'results/hardware-e-02' if group == 'modes' else 'results/hardware-e-01'
        hardware_path = root / run / name / 'hardware.rdram.bin'
        hardware = hardware_path.read_bytes()
        (destination / f'{name}.hardware.bin').write_bytes(hardware)
        vertices = state.get('vertices')
        if group == 'timing':
            sample = meta['timing_predictions'][0]['inputs']
            vertices = [[x, y, sample['combiner_rgba'][:3] + [state['shade_at_8_8'] + (x - 8) * state['da_dx']]] for x, y in [(2, 2), (30, 2), (2, 22)]]
        if group == 'matrix':
            rgba = state['rgba']
            triangles = byte_triangles([[4, 4, rgba], [28, 4, rgba], [28, 20, rgba]]) + byte_triangles([[4, 4, rgba], [28, 20, rgba], [4, 20, rgba]])
        else:
            triangles = byte_triangles(vertices)
        ops = []
        for words in commands(path.with_suffix('.rdpdump').read_bytes()):
            opcode = (words[0] >> 24) & 63
            if 8 <= opcode <= 15:
                ops.append({'triangles': triangles})
                continue
            assert len(words) == 2
            words[0] |= 0xc0000000
            if opcode == 63:
                address = words[1] - 0x100000
                words[1] = 0x100000 if address == 0 else 0x200000 if address in (0x4000, 0x5000) else 0x300000 + address
                if address in (0, 0x4000, 0x5000):
                    words[0] = (words[0] & ~1023) | 319
            elif opcode == 62:
                words[1] = 0x200000
            elif opcode == 61:
                words[1] += 0x200000
            ops.append({'command': words})
        width, height = meta['width'], meta['height']
        mask = bytearray(width * height)
        for y in range(height):
            for x in range(width):
                i = y * width + x
                if group == 'matrix':
                    rect = meta['compare'][0]['rect']
                    mask[i] = int(rect[0] <= x < rect[2] and rect[1] <= y < rect[3] and ((rect[2] - rect[0]) * (rect[3] - rect[1]) == 1 or abs((x - 4) * 16 - (y - 4) * 24) > 40))
                elif group == 'timing':
                    mask[i] = int(y == 8 and 7 <= x <= 10)
                elif group == 'modes':
                    mask[i] = 1 if geometry[i] == 8 and edge_distance(vertices, x, y) > 1.5 else 2 if geometry[i] else 0
                else:
                    mask[i] = 1 if hardware[i * 4 + 3] >> 5 == 7 and edge_distance(vertices, x, y) > 0.5 else 2 if edge_distance(vertices, x, y) <= 1.5 else 0
        (destination / f'{name}.mask.bin').write_bytes(mask)
        cases.append(dict(id=name, group=group, width=width, height=height, format=meta['format'], ops=ops,
                          state=state, hardware_run=run, hardware_sha256=digest(hardware), mask_sha256=digest(mask),
                          source_json=str(path.relative_to(root)), source_json_sha256=digest(path.read_bytes()),
                          source_dump_sha256=digest(path.with_suffix('.rdpdump').read_bytes()), triangles=len(triangles),
                          interiors=mask.count(1), edges=mask.count(2),
                          input_conversion='RGBA rounded to bytes; XY rounded to 1/128 pixel; out-of-range planes split at nine-bit clamp/wrap boundaries' if group in ('interp', 'timing') else 'exact byte RGBA; exact quarter-pixel XY'))
    (destination / 'cases.json').write_text(json.dumps(dict(fast3d='8339189', rt64='43373749dac9bbc1b653e6a02aed40a9e1783bed', angrylion='9c8b9ed3e7d7f00dff8bc872ccdd3fba1a3673fc', parallel='1cecd042b2619bc505c12bfdc713808386f2b54d', cases=cases), separators=(',', ':')) + '\n')
    print(f'{len(cases)} cases, {sum(c["interiors"] for c in cases)} interior samples')


if __name__ == '__main__':
    parser = argparse.ArgumentParser()
    parser.add_argument('oracle', type=Path)
    parser.add_argument('destination', type=Path)
    args = parser.parse_args()
    with tempfile.TemporaryDirectory() as directory:
        source = Path(directory)
        generate(args.oracle, source)
        model.generate(source)
        compact.compact(source, args.destination)
