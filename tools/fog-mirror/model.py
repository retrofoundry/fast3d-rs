import json
import hashlib
from pathlib import Path
import sys


def plane(triangle, x, y):
    a, b, c = triangle
    ax, ay, bx, by, cx, cy = a[0] / 128, a[1] / 128, b[0] / 128, b[1] / 128, c[0] / 128, c[1] / 128
    area = (bx - ax) * (cy - ay) - (by - ay) * (cx - ax)
    if abs(area) < 1e-10:
        return None
    v = ((x - ax) * (cy - ay) - (y - ay) * (cx - ax)) / area
    w = ((bx - ax) * (y - ay) - (by - ay) * (x - ax)) / area
    u = 1 - v - w
    if min(u, v, w) < -1e-8:
        return None
    return [(u * a[k] + v * b[k] + w * c[k]) / 255 for k in range(2, 6)]


def combiner(words, cycles, shade, prim, env, texel):
    l, h = words
    combined = [0.] * 4
    compare_alpha = None
    for cycle in ([1] if cycles == 0 else [0, 1]):
        if cycle == 0:
            ca, cb, cc, cd = (l >> 20) & 15, h >> 28, (l >> 15) & 31, (h >> 15) & 7
            aa, ab, ac, ad = (l >> 12) & 7, (h >> 12) & 7, (l >> 9) & 7, (h >> 9) & 7
        else:
            ca, cb, cc, cd = (l >> 5) & 15, (h >> 24) & 15, l & 31, (h >> 6) & 7
            aa, ab, ac, ad = (h >> 21) & 7, (h >> 3) & 7, (h >> 18) & 7, h & 7
        values = [combined, texel, texel, prim, shade, env, [1.] * 4, [0.] * 4]
        alpha_c = [1., texel[3], texel[3], prim[3], shade[3], env[3], 0., 0.]
        color_c = values[:6] + [[0.] * 4, [0.] * 4] + [[values[k][3]] * 4 for k in (1, 2, 3, 4, 5)] + [[1.] * 4, [0.] * 4]
        out = []
        for channel in range(3):
            a = values[ca][channel] if ca < 7 else 0.
            b = values[cb][channel] if cb < 6 else 0.
            c = color_c[cc][channel] if cc < len(color_c) else 0.
            d = values[cd][channel]
            out.append(max(0., min(1., (a - b) * c + d)))
        out.append(max(0., min(1., (values[aa][3] - values[ab][3]) * alpha_c[ac] + values[ad][3])))
        combined = out
        if compare_alpha is None:
            compare_alpha = out[3]
    return combined, compare_alpha


def modern(case, shade, x, y):
    state = case['state']
    if case['id'] == 'shade-next-forward':
        alpha = (state['shade_at_8_8'] + (x - 8) * state['da_dx']) & 511
        return [alpha if alpha < 256 else 255 if alpha < 384 else 0] * 3
    high, low = state['othermode_h'], state['othermode_l']
    cycles = (high >> 20) & 3
    prim = [c / 255 for c in state.get('primitive_rgba', [0, 0, 0, 0])]
    env = [c / 255 for c in state.get('environment_rgba', [0, 0, 0, 0])]
    fog = [c / 255 for c in state.get('fog_rgba', [0, 0, 0, 0])]
    words = state.get('combine', state.get('raw_combine_words'))
    texel = [((v << 3) | (v >> 2)) / 255 for v in [15, 18, 30]] + [1.] if case['group'] == 'modes' else [1.] * 4
    color, compare_alpha = combiner(words, cycles, shade, prim, env, texel)
    if low & 3 == 1 and compare_alpha < state.get('blend_rgba', [0, 0, 0, 128])[3] / 255:
        return [24, 72, 120]
    p, a, m, b = [(low >> shift) & 3 for shift in (30, 26, 22, 18)]
    blend = cycles == 1 or bool(low & 0x4000)
    local = (p, m) in ((0, 3), (3, 0))
    if local:
        assert a == 2 and b == 0, case['id']
        if p == 3 or blend:
            t = shade[3] if p == 3 else 1 - shade[3]
            color[:3] = [rgb * (1 - t) + f * t for rgb, f in zip(color[:3], fog[:3])]
    if case['group'] == 'modes':
        shifts = (28, 24, 20, 16) if cycles == 1 else (30, 26, 22, 18)
        fp, fa, fm, fb = [(low >> shift) & 3 for shift in shifts]
        active = (fp == 1 and fa != 3) or (bool(low & 0x4000) and fm == 1 and fb != 3)
        if active:
            pixel = state['background']['rgba16'][(x // 16 + y // 4) & 1]
            memory = [((pixel >> shift & 31) * 8 + (pixel >> shift & 31) // 4) / 255 for shift in (11, 6, 1)]
            alpha = [color[3], fog[3], shade[3], 0.][fa]
            color[:3] = [a * alpha + b * (1 - alpha) for a, b in zip(color, memory)]
    return [max(0, min(255, round(c * 255))) for c in color[:3]]


def generate(directory):
    manifest = json.loads((directory / 'cases.json').read_text())
    cases = manifest['cases']
    for case in cases:
        selected = (case['group'] == 'interp' or case['id'] in ('compare-next-forward', 'shade-next-forward')
                    or (case['group'] == 'modes' and case['state']['othermode_l'] & 0xc00 != 0xc00)
                    or (case['group'] == 'matrix' and int(case['id'][4:6]) in (23, 29, 30, 31, 33, 34, 39, 40)))
        if not selected:
            (directory / f'{case["id"]}.modern.bin').unlink(missing_ok=True)
            case.pop('modern_sha256', None)
            continue
        mask = (directory / f'{case["id"]}.mask.bin').read_bytes()
        output = bytearray()
        triangles = next(op['triangles'] for op in case['ops'] if 'triangles' in op)
        for i, selected in enumerate(mask):
            if selected != 1:
                continue
            x, y = i % case['width'], i // case['width']
            shade = next((value for tri in triangles if (value := plane(tri, x, y)) is not None), None)
            if shade is None:
                raise ValueError((case['id'], x, y))
            output.extend(modern(case, shade, x, y))
        if case['interiors']:
            (directory / f'{case["id"]}.modern.bin').write_bytes(output)
            case['modern_sha256'] = hashlib.sha256(output).hexdigest()
        else:
            (directory / f'{case["id"]}.modern.bin').unlink(missing_ok=True)
            case.pop('modern_sha256', None)
        if case['interiors'] == 0:
            import generate as geometry
            edge = bytearray()
            triangle = triangles[0]
            points = [[v[0] / 128, v[1] / 128] for v in triangle]
            for i, selected in enumerate(mask):
                if selected != 2:
                    continue
                x, y = i % case['width'], i // case['width']
                shade = plane(triangle, x, y)
                expected = modern(case, shade, x, y) if shade is not None else [17, 23, 31]
                edge.extend(expected + [int(geometry.edge_distance(points, x, y) > 1 / 128)])
            (directory / f'{case["id"]}.edge-modern.bin').write_bytes(edge)
            case['edge_modern_sha256'] = hashlib.sha256(edge).hexdigest()
    manifest['model_source_sha256'] = hashlib.sha256(Path(__file__).read_bytes()).hexdigest()
    manifest['generator_source_sha256'] = hashlib.sha256(Path(__file__).with_name('generate.py').read_bytes()).hexdigest()
    (directory / 'cases.json').write_text(json.dumps(manifest, separators=(',', ':')) + '\n')


if __name__ == '__main__':
    generate(Path(sys.argv[1]))
