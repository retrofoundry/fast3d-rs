"""Derive coverage and ownership from frozen IMAGE bytes, without renderer output."""

import argparse
import hashlib
import json
import math
import struct
from pathlib import Path


def f32(x):
    return struct.unpack("f", struct.pack("f", x))[0]


def identity():
    return [[float(i == j) for j in range(4)] for i in range(4)]


def dot(a, b):
    value = 0.0
    for x, y in zip(a, b):
        value = f32(value + f32(x * y))
    return value


def multiply(a, b):
    return [[dot(row, col) for col in zip(*b)] for row in a]


def primitives(data, config):
    triangles = []
    matrices = []
    for task in config["tasks"]:
        f3d = task["microcode"] == "f3d"
        segments = task["segments"]
        address = lambda v: segments[v >> 24] + (v & 0xFFFFFF)
        model, projection = identity(), identity()
        viewport = [160, 120, 160, 120]
        slots = {}
        stack = [task["entry"]]
        while stack:
            pc = stack[-1]
            w0, w1 = struct.unpack_from(">II", data, pc)
            stack[-1] += 8
            op = w0 >> 24
            if op == (0xB8 if f3d else 0xDF):
                stack.pop()
            elif op == (0x06 if f3d else 0xDE):
                stack.append(address(w1))
            elif op == (0x01 if f3d else 0xDA):
                at = address(w1)
                hi = struct.unpack_from(">16h", data, at)
                lo = struct.unpack_from(">16H", data, at + 32)
                values = [h + l / 65536 for h, l in zip(hi, lo)]
                m = [values[i:i + 4] for i in range(0, 16, 4)]
                flags = (w0 >> 16) & 255 if f3d else (w0 & 255) ^ 1
                is_projection = bool(flags & (1 if f3d else 4))
                if is_projection:
                    projection = m if flags & 2 else multiply(m, projection)
                else:
                    model = m if flags & 2 else multiply(m, model)
                matrices.append({"pc": pc, "address": at, "values": values})
            elif (f3d and op == 3 and (w0 >> 16) & 255 == 128) or (not f3d and op == 0xDC and w0 & 255 == 8):
                vp = struct.unpack_from(">8h", data, address(w1))
                viewport = [vp[0] / 4, vp[1] / 4, vp[4] / 4, vp[5] / 4]
            elif op == (4 if f3d else 1):
                count = ((w0 >> 20) & 15) + 1 if f3d else (w0 >> 12) & 255
                first = (w0 >> 16) & 15 if f3d else ((w0 >> 1) & 127) - count
                mvp = multiply(model, projection)
                for i in range(count):
                    at = address(w1) + i * 16
                    raw = struct.unpack_from(">3h", data, at)
                    clip = [dot([*raw, 1], col) for col in zip(*mvp)]
                    assert clip[3] > 0, "these prediction inputs must not cross w=0"
                    sx, sy, tx, ty = viewport
                    x = clip[0] / clip[3] * sx + tx
                    y = -clip[1] / clip[3] * sy + ty
                    if "color_image" not in config:
                        x *= config["width"] / 320
                        y *= config["height"] / 240
                    slots[first + i] = {"xy": [x, y], "clip": clip, "raw": raw, "address": at}
            elif (f3d and op == 0xBF) or (not f3d and op in (5, 6, 7)):
                words = [w1] if f3d else ([w0, w1] if op in (6, 7) else [w0])
                for word in words:
                    ids = [((word >> shift) & 255) // (10 if f3d else 2) for shift in (16, 8, 0)]
                    triangles.append({"pc": pc, "vertices": [slots[i] for i in ids]})
    return triangles, matrices


def edge(a, b, p):
    return (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0])


def owners(triangles, width, height, offset, snap=None, depth=False):
    result = [-1] * (width * height)
    zbuffer = [math.inf] * (width * height)
    for i, triangle in enumerate(triangles):
        points = [v["xy"] for v in triangle["vertices"]]
        z = [v["clip"][2] / v["clip"][3] for v in triangle["vertices"]]
        if snap:
            points = [[round(x * snap) / snap, round(y * snap) / snap] for x, y in points]
        area = edge(*points)
        if area == 0:
            continue
        if area < 0:
            points[1], points[2] = points[2], points[1]
            z[1], z[2] = z[2], z[1]
            area = -area
        edges = list(zip(points, points[1:] + points[:1]))
        for y in range(max(0, math.floor(min(p[1] for p in points))), min(height, math.ceil(max(p[1] for p in points)) + 1)):
            for x in range(max(0, math.floor(min(p[0] for p in points))), min(width, math.ceil(max(p[0] for p in points)) + 1)):
                p = [x + offset, y + offset]
                if all((e := edge(a, b, p)) > 0 or e == 0 and (b[1] < a[1] or b[1] == a[1] and b[0] > a[0]) for a, b in edges):
                    index = y * width + x
                    value = sum(edge(points[(j+1)%3], points[(j+2)%3], p) * z[j] for j in range(3)) / area
                    if not depth or value <= zbuffer[index]:
                        result[index] = i
                        zbuffer[index] = value
    return result


def mask_file(path, mask, width):
    raw = b"".join(bytes([255, 255, 255, 255] if bit else [0, 0, 0, 255]) for bit in mask)
    path.write_bytes(raw)
    points = [(i % width, i // width) for i, bit in enumerate(mask) if bit]
    bounds = [min(x for x, _ in points), min(y for _, y in points), max(x for x, _ in points), max(y for _, y in points)] if points else None
    return {"pixels": len(points), "bounds_inclusive": bounds, "sha256": hashlib.sha256(raw).hexdigest()}


def predict(rdram, meta, output):
    data = rdram.read_bytes()
    config = json.loads(meta.read_text())
    known = {
        "c464b37f0a6d288635922a8d27d530119e1abd175d02f26856ee598b76955ced": (64, 64),
        "64cddffe44d2a0c276ab3981a0c0f30cd384f45ea90aef986f7330c38e5170b2": (320, 240),
        "e069af71157859e37e03fe901732b375c41f75c790688d8787b47b9486aad4bc": (320, 240),
    }
    digest = hashlib.sha256(data).hexdigest()
    assert digest in known, "derive command/state/attribute support before adding another input"
    width, height = config["width"], config["height"]
    assert (width, height) == known[digest]
    triangles, matrices = primitives(data, config)
    depth = "color_image" not in config
    a, b = [owners(triangles, width, height, d, depth=depth) for d in (0.5, 0)]
    silhouette = [(x >= 0) != (y >= 0) for x, y in zip(a, b)]
    ownership = [x >= 0 and y >= 0 and x != y for x, y in zip(a, b)]
    output.parent.mkdir(parents=True, exist_ok=True)
    stats = {"input_sha256": hashlib.sha256(data).hexdigest(), "config_sha256": hashlib.sha256(meta.read_bytes()).hexdigest(), "triangles": len(triangles), "width": width, "height": height}
    for name, mask in [("silhouette", silhouette), ("ownership", ownership), ("support-a", [x >= 0 for x in a]), ("support-b", [x >= 0 for x in b])]:
        stats[name] = mask_file(Path(f"{output}.{name}.rgba8"), mask, width)
    # Sphere has constant texture output; the two IMAGE controls have identical attribute planes across owners.
    stats["byte-delta"] = mask_file(Path(f"{output}.byte-delta.rgba8"), silhouette, width)
    if "sphere" in output.name:
        stats["subpixel_snap_support_disagreements"] = {
            str(snap): [sum((i >= 0) != (j >= 0) for i, j in zip(o, owners(triangles, width, height, d, snap))) for o, d in [(a, 0.5), (b, 0)]] for snap in (16, 256)
        }
    Path(f"{output}.geometry.json").write_text(json.dumps({"matrices": matrices, "triangles": triangles}, indent=2) + "\n")
    Path(f"{output}.prediction.json").write_text(json.dumps(stats, indent=2) + "\n")
    print(output.name, json.dumps(stats))
    return stats


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("rdram", type=Path)
    parser.add_argument("config", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    predict(args.rdram, args.config, args.output)
