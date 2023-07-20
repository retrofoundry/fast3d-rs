#!/bin/sh

set -e

cd fast3d && wasm-pack test --headless --firefox && cd ..
