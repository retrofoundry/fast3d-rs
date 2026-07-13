{ pkgs, lib, ... }:
{
  languages.rust = {
    enable = true;
    toolchainFile = ./rust-toolchain.toml;
  };

  languages.javascript = {
    enable = true;
    package = pkgs.nodejs_22;
    pnpm.enable = true;
  };

  packages = [ pkgs.wasm-bindgen-cli_0_2_108 ];

  scripts.build-wasm.exec = ''
    cargo build -p web --target wasm32-unknown-unknown --release
    wasm-bindgen target/wasm32-unknown-unknown/release/web.wasm \
      --target web --out-dir web-app/src/wasm --out-name n64_toys
  '';

  scripts.dev.exec = ''
    test -d web-app || { echo "web-app not scaffolded; run the Svelte scaffold task first" >&2; exit 1; }
    build-wasm
    cd web-app && pnpm install && pnpm run dev
  '';
}
