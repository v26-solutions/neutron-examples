{
  description = "Rust Development Shell";

  inputs = {
    nagy-nur.url = "github:nagy/nur-packages";
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    nixpkgs.url      = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url  = "github:numtide/flake-utils";
  };

  outputs = { self, nagy-nur, fenix, nixpkgs, flake-utils, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ fenix.overlays.default ];
        pkgs = import nixpkgs {
          inherit system overlays;
        };
        toolchain = with fenix.packages.${system}; combine [
            (complete.withComponents [
              "cargo"
              "clippy"
              "rust-src"
              "rustc"
              "rustfmt"
              "llvm-tools-preview"
            ])          
            targets.wasm32-unknown-unknown.latest.rust-std
        ];
      in
      with pkgs;
      {
        devShells.default = mkShell {
          buildInputs = [
            toolchain
            rust-analyzer-nightly
            taplo
            cargo-expand
            cargo-llvm-cov
            cargo-watch
            binaryen
            twiggy
            nagy-nur.packages.${system}.rustfilt
            go
          ];
        };
      }
    );
}
