{ pkgs, lib, ... }:
{
  # fast3d is a pure Rust library; the wasm32 target + clippy/rustfmt come from
  # rust-toolchain.toml. (The web playground + its JS/wasm-bindgen tooling live in the
  # consumer app, not here.)
  languages.rust = {
    enable = true;
    toolchainFile = ./rust-toolchain.toml;
  };
}
