#!/usr/bin/env python3
import argparse
import http.server
import json
import re
from pathlib import Path

parser = argparse.ArgumentParser()
parser.add_argument('--build', type=Path, required=True)
parser.add_argument('--scene', type=Path, required=True)
parser.add_argument('--out', type=Path, required=True)
parser.add_argument('--port', type=int, default=8765)
parser.add_argument('--cases', type=Path)
parser.add_argument('--capture', type=Path)
args = parser.parse_args()
args.out.mkdir(parents=True, exist_ok=True)
class Handler(http.server.BaseHTTPRequestHandler):
    def send(self, data, mime):
        self.send_response(200)
        self.send_header('Content-Type', mime)
        self.send_header('Cross-Origin-Opener-Policy', 'same-origin')
        self.send_header('Cross-Origin-Embedder-Policy', 'require-corp')
        self.end_headers()
        self.wfile.write(data)
    def do_GET(self):
        if self.path == '/':
            self.send(Path(__file__).with_name('index.html').read_bytes(), 'text/html')
        elif self.path == '/scene.n64':
            self.send(args.scene.read_bytes(), 'text/plain')
        elif self.path == '/cases.json' and args.cases:
            self.send(args.cases.read_bytes(), 'application/json')
        elif self.path == '/capture.f3dcap' and args.capture:
            self.send(args.capture.read_bytes(), 'application/octet-stream')
        elif re.fullmatch(r'/pkg/[a-zA-Z0-9_.-]+', self.path):
            path = args.build / self.path[1:]
            self.send(path.read_bytes(), 'application/wasm' if path.suffix == '.wasm' else 'text/javascript')
        else:
            self.send_error(404)
    def do_POST(self):
        if not re.fullmatch(r'/artifact/[a-zA-Z0-9_.-]+\.json', self.path):
            self.send_error(400)
            return
        data = self.rfile.read(int(self.headers['Content-Length']))
        value = json.loads(data)
        path = args.out / self.path.rsplit('/', 1)[1]
        path.write_bytes(data)
        if 'frames' in value:
            path.with_suffix('.jsonl').write_text(''.join(json.dumps(row)+'\n' for row in value['frames']))
        if 'costs' in value:
            path.with_suffix('.jsonl').write_text(''.join(json.dumps(row)+'\n' for row in value['costs']))
        self.send(str(path).encode(), 'text/plain')
http.server.ThreadingHTTPServer(('127.0.0.1', args.port), Handler).serve_forever()
