import hashlib
import json
import struct


def spans(indices):
    result = []
    for i in indices:
        if result and sum(result[-1]) == i:
            result[-1][1] += 1
        else:
            result.append([i, 1])
    return result


def bug(case):
    if case['group'] == 'matrix':
        return {16: 'memory_alpha_and_coverage', 17: 'memory_alpha_and_coverage',
                18: 'memory_alpha_and_coverage', 20: 'color_on_coverage',
                22: 'partial_reject'}.get(int(case['id'][4:6]))
    return {'memory-previous-forward': 'first_cycle_memory_rgb',
            'coverage-previous-forward': 'memory_alpha_and_coverage'}.get(case['id'])


def compact(source, destination):
    manifest = json.loads((source / 'cases.json').read_text())
    mode_ops = [c['ops'] for c in manifest['cases'] if c['group'] == 'modes']
    common = []
    for ops in zip(*mode_ops):
        if any(op != ops[0] for op in ops):
            break
        common.append(ops[0])
    destination.mkdir(parents=True, exist_ok=True)
    ledger = destination / 'provenance.json'
    previous = json.loads(ledger.read_text())['files'] if ledger.exists() else {}
    assets = {}
    cases, provenance = [], []
    for original in manifest['cases']:
        name = original['id']
        case = {key: original[key] for key in ('id', 'group', 'width', 'height', 'format')}
        case['ops'] = original['ops'][len(common):] if case['group'] == 'modes' else original['ops']
        case['ci'] = bool(original['state']['othermode_h'] & 0x8000)
        case['bug'] = bug(original)
        raw = {}
        for suffix, key in [('hardware.bin', 'hardware_sha256'), ('mask.bin', 'mask_sha256'),
                            ('modern.bin', 'modern_sha256'), ('edge-modern.bin', 'edge_modern_sha256')]:
            if key in original:
                data = (source / f'{name}.{suffix}').read_bytes()
                assert hashlib.sha256(data).hexdigest() == original[key], (name, key)
                raw[suffix] = data
        indices = [i for i, value in enumerate(raw['mask.bin']) if value == 1]
        modern = raw.get('modern.bin', b'')
        thin = not indices
        if thin:
            assert name.startswith('interp-thin-')
            edge_indices = [i for i, value in enumerate(raw['mask.bin']) if value == 2]
            edge = raw['edge-modern.bin']
            assert len(edge) == len(edge_indices) * 4
            indices = [i for j, i in enumerate(edge_indices) if edge[j * 4 + 3]]
            modern = b''.join(edge[j * 4:j * 4 + 3] for j in range(len(edge_indices)) if edge[j * 4 + 3])
        assert indices
        assert not modern or len(modern) == len(indices) * 3
        case['spans'] = spans(indices)
        case['samples'] = len(indices)
        case['region'] = 'edge' if thin else 'interior'
        case['modern_tolerance'] = int(case['group'] in ('modes', 'interp'))
        case['hardware_tolerance'] = 0
        if modern:
            number = int(name[4:6]) if case['group'] == 'matrix' else None
            case['hardware_tolerance'] = (2 if case['group'] == 'modes' else
                                          255 if name == 'shade-next-forward' else
                                          80 if name == 'compare-next-forward' else
                                          {23: 6, 29: 5, 31: 5, 33: 5, 39: 7}.get(number, 1))
        case['precision'] = ('edge rasterization and byte vertex adaptation' if thin else
                             'float mix and unpatterned dither' if case['group'] in ('matrix', 'modes') else
                             'affine float interpolation and byte vertex adaptation' if case['group'] == 'interp' else
                             'current pixel timing') if modern else None
        size = 2 if case['format'] == 'rgba16' else 4
        records = [raw['hardware.bin'][i * size:(i + 1) * size] + modern[j * 3:j * 3 + 3]
                   for j, i in enumerate(indices)]
        palette = list(dict.fromkeys(records))
        if thin:
            case['hardware_tolerance'] = max(abs(record[k] - record[size + k])
                                             for record in records for k in range(3)) + case['modern_tolerance']
        case['modern'] = bool(modern)
        if len(palette) == 1:
            case['hardware_word'] = int.from_bytes(palette[0][:size], 'big')
            if modern:
                case['modern_rgb'] = list(palette[0][size:])
        else:
            lookup = {value: i for i, value in enumerate(palette)}
            data = struct.pack('<H', len(palette)) + b''.join(palette)
            data += struct.pack(f'<{len(records)}H', *(lookup[value] for value in records))
            case['data'] = assets.setdefault(data, f'{name}.bin')
            (destination / case['data']).write_bytes(data)
        cases.append(case)
        provenance.append({key: original[key] for key in
                           ('id', 'hardware_run', 'hardware_sha256', 'mask_sha256', 'source_json',
                            'source_json_sha256', 'source_dump_sha256')})
    for name in previous:
        if name.endswith('.bin') and name not in assets.values():
            (destination / name).unlink(missing_ok=True)
    (destination / 'cases.json').write_text(json.dumps(dict(common_ops=common, cases=cases), separators=(',', ':')) + '\n')
    provenance = dict(generators={key: manifest[key] for key in ('generator_source_sha256', 'model_source_sha256')},
                      revisions={key: manifest[key] for key in ('ultragfx', 'rt64', 'angrylion', 'parallel')},
                      input_conversion={group: next(c['input_conversion'] for c in manifest['cases'] if c['group'] == group)
                                        for group in ('matrix', 'modes', 'interp', 'timing')},
                      cases=provenance,
                      files={name: hashlib.sha256((destination / name).read_bytes()).hexdigest()
                             for name in sorted(set(assets.values()) | {'cases.json'})})
    (destination / 'provenance.json').write_text(json.dumps(provenance, separators=(',', ':')) + '\n')
    print(f'{len(cases)} compact cases, {sum(p.stat().st_size for p in destination.iterdir() if p.is_file())} bytes')
