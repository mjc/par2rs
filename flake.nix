{
  description = "Rust development environment";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs = {
    self,
    nixpkgs,
    flake-utils,
    rust-overlay,
  }:
    flake-utils.lib.eachDefaultSystem (
      system: let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [(import rust-overlay)];
        };
        # Read the file relative to the flake's root
        overrides = builtins.fromTOML (builtins.readFile (self + "/rust-toolchain.toml"));
        libPath = with pkgs;
          lib.makeLibraryPath [
            # load external libraries that you need in your rust project here
          ];
      in {
        # Package output - can be used as flake input and drop-in replacement for par2cmdline
        packages = {
          default = self.packages.${system}.par2rs;

          par2rs = let
            rustNightly = pkgs.rust-bin.nightly.latest.default.override {
              extensions = ["rust-src"];
            };
          in
            (pkgs.makeRustPlatform {
              cargo = rustNightly;
              rustc = rustNightly;
            })
            .buildRustPackage {
              pname = "par2rs";
              version = "0.1.0";

              src = ./.;

              cargoLock = {
                lockFile = ./Cargo.lock;
              };

              # The main binary is 'par2' which provides par2cmdline-compatible interface
              # Additional utilities: par2verify, par2repair, par2create, split_par2
              meta = with pkgs.lib; {
                description = "High-performance PAR2 file verification and repair utility written in Rust";
                longDescription = ''
                  par2rs is a Rust implementation of the PAR2 (Parity Archive) specification,
                  providing fast file verification and repair capabilities. It can be used as
                  a drop-in replacement for par2cmdline with improved performance.

                  The main 'par2' binary provides a compatible command-line interface:
                  - par2 create (or 'c') - Create PAR2 recovery files
                  - par2 verify (or 'v') - Verify file integrity
                  - par2 repair (or 'r') - Repair damaged files
                '';
                homepage = "https://github.com/mjc/par2rs";
                license = licenses.mit;
                maintainers = [];
                mainProgram = "par2";
              };
            };
        };

        devShells.default = pkgs.mkShell rec {
          nativeBuildInputs = [pkgs.pkg-config pkgs.direnv];
          buildInputs = with pkgs;
            [
              clang
              llvmPackages.bintools
              rustup
              openssl
              openssl.dev
              pkg-config
              par2cmdline
              xxd
              python3 # for copilot
              rust-analyzer
              cargo-watch
              rustfmt
              clippy
              yamllint
              cargo-flamegraph
              bc # for benchmark averaging calculations
              gnuplot # for criterion benchmark graphs
              gh # GitHub CLI
              git-filter-repo # for rewriting git history
            ]
            ++ pkgs.lib.optionals pkgs.stdenv.isLinux [
              heaptrack
              perf
            ];

          RUSTC_VERSION = overrides.toolchain.channel;

          # https://github.com/rust-lang/rust-bindgen#environment-variables
          LIBCLANG_PATH = pkgs.lib.makeLibraryPath [pkgs.llvmPackages_latest.libclang.lib];

          shellHook = ''
            export PATH=$PATH:''${CARGO_HOME:-~/.cargo}/bin
            export PATH=$PATH:''${RUSTUP_HOME:-~/.rustup}/toolchains/$RUSTC_VERSION-${
              if pkgs.stdenv.isDarwin
              then
                if pkgs.stdenv.isAarch64
                then "aarch64-apple-darwin"
                else "x86_64-apple-darwin"
              else "x86_64-unknown-linux-gnu"
            }/bin/
          '';

          # Add precompiled library to rustc search path
          RUSTFLAGS = builtins.map (a: ''-L ${a}/lib'') [
            # add libraries here (e.g. pkgs.libvmi)
          ];

          LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath (buildInputs ++ nativeBuildInputs);

          # Add glibc, clang, glib, and other headers to bindgen search path
          BINDGEN_EXTRA_CLANG_ARGS =
            # Includes normal include path
            (builtins.map (a: ''-I"${a}/include"'') (
              # add dev libraries here (e.g. pkgs.libvmi.dev)
              pkgs.lib.optionals pkgs.stdenv.isLinux [pkgs.glibc.dev]
            ))
            # Includes with special directory paths
            ++ [
              ''-I"${pkgs.llvmPackages_latest.libclang.lib}/lib/clang/${pkgs.llvmPackages_latest.libclang.version}/include"''
            ]
            ++ pkgs.lib.optionals pkgs.stdenv.isLinux [
              ''-I"${pkgs.glib.dev}/include/glib-2.0"''
              ''-I${pkgs.glib.out}/lib/glib-2.0/include/''
            ];
        };
      }
    );
}
