{
  description = "Magical Crypto Wallet flake";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
  outputs = { self, nixpkgs }:
    let
        pkgs = import nixpkgs { system = "x86_64-linux"; config.permittedInsecurePackages = ["python3.13-ecdsa-0.19.1"]; };
        pkgsUnfree = import nixpkgs { system = "x86_64-linux"; config.permittedInsecurePackages = ["python3.13-ecdsa-0.19.1"]; config.allowUnfree = true; };
        deployScript = pkgs.writeScriptBin "deploy" (builtins.readFile ./Contrib/deploy.sh);
        secpArchive = pkgs.fetchurl {
          url = "https://codeload.github.com/bitcoin-core/secp256k1/tar.gz/refs/tags/v0.7.1";
          sha256 = "958f204dbafc117e73a2604285dc2eb2a5128344d3499c114dcba5de54cb7a9e";
        };
        secpSource = pkgs.runCommand "secp256k1-source" {} ''
          mkdir -p $out
          tar -xf ${secpArchive} --strip-components=1 -C $out
        '';
        nativeCredentials = pkgs.stdenv.mkDerivation {
          pname = "magicalcryptowallet-credentials";
          version = "1.3.1";
          src = ./ThirdParty/WabiSabi/c;
          nativeBuildInputs = [ pkgs.cmake ];
          cmakeFlags = [ "-DFETCHCONTENT_SOURCE_DIR_SECP256K1=${secpSource}" ];
          doCheck = true;
          checkPhase = "ctest --output-on-failure";
          installPhase = "install -Dm755 libwabisabi.so $out/lib/libwabisabi.so";
        };
        gitRev = if (builtins.hasAttr "rev" self) then self.rev else "dirty";
        # Official, checksum-pinned compiler/std distribution. Compiler runtimes
        # are build tools; only the separately audited mcw application ships.
        rustToolchain = pkgs.stdenv.mkDerivation {
          pname = "mcw-rust-toolchain";
          version = "1.99.0";
          srcs = [
            (pkgs.fetchurl { url = "https://static.rust-lang.org/dist/2026-10-01/rustc-1.99.0-x86_64-unknown-linux-gnu.tar.xz"; sha256 = "77171ba2a0345fdf2abc4fedda55d6de078dae7a68527c28be8c77dcc9604bd5"; })
            (pkgs.fetchurl { url = "https://static.rust-lang.org/dist/2026-10-01/cargo-1.99.0-x86_64-unknown-linux-gnu.tar.xz"; sha256 = "d7674918d28093097614cd9728b6ca60db9ea3038f640f0bd1e9a4188c7568ce"; })
            (pkgs.fetchurl { url = "https://static.rust-lang.org/dist/2026-10-01/rust-std-1.99.0-x86_64-unknown-linux-gnu.tar.xz"; sha256 = "3e58dff2d0b72196b5ea4e90536e174d400de88564a52694686b81e091169933"; })
            (pkgs.fetchurl { url = "https://static.rust-lang.org/dist/2026-10-01/rust-src-1.99.0.tar.xz"; sha256 = "3f1f9b7ed48f4596fc87889b7b3c61747336a55c9c22db1ab0c697e0aadb77aa"; })
          ];
          sourceRoot = ".";
          nativeBuildInputs = [ pkgs.autoPatchelfHook ];
          buildInputs = [ pkgs.stdenv.cc.cc.lib pkgs.zlib pkgs.openssl ];
          dontConfigure = true;
          dontBuild = true;
          dontStrip = true;
          installPhase = ''
            patchShebangs --build ./*/install.sh
            for component in rustc cargo rust-std; do
              ./$component-1.99.0-x86_64-unknown-linux-gnu/install.sh --prefix=$out --disable-ldconfig
            done
            ./rust-src-1.99.0/install.sh --prefix=$out --disable-ldconfig
          '';
        };
        rustStdVendor = import ./Contrib/Mcw/rust-std-vendor.nix { inherit pkgs; };
        mcwHost = pkgs.stdenv.mkDerivation {
          pname = "mcw";
          version = "99.99.99";
          src = ./mcw;
          nativeBuildInputs = [ rustToolchain ];
          buildPhase = ''
            export CARGO_HOME=$TMPDIR/mcw-cargo
            export MCW_VERSION=99.99.99
            export RUSTC_BOOTSTRAP=1
            export RUSTFLAGS="-C panic=abort -C default-linker-libraries=no"
            mcwShippingLinker="${pkgs.writeShellScript "mcw-link-linux" (builtins.readFile ./Contrib/Mcw/link-linux.sh)}"
            mkdir -p .cargo
            cat > .cargo/config.toml <<EOF
            [source.crates-io]
            replace-with = "compiler-std"
            [source.compiler-std]
            directory = "${rustStdVendor}"
            EOF
            cargo -Z build-std=std,panic_abort -Z build-std-features= rustc --target x86_64-unknown-linux-gnu --release --locked --offline --bin mcw -- -C "linker=$mcwShippingLinker"
          '';
          doCheck = true;
          checkPhase = ''
            unset RUSTFLAGS RUSTC_BOOTSTRAP
            cargo test --locked --offline
            if readelf -d target/x86_64-unknown-linux-gnu/release/mcw | grep -E 'NEEDED.*(libgcc|libstdc|libssl|libcrypto)'; then exit 1; fi
          '';
          installPhase = "install -Dm755 target/x86_64-unknown-linux-gnu/release/mcw $out/bin/mcw";
        };
        buildMagicalCryptoWalletModule = pkgs.buildDotnetModule ({
          pname = "magicalcryptowallet";
          version = "2.0.0-${builtins.substring 0 8 (self.lastModifiedDate or self.lastModified or "19700101")}-${gitRev}";
          nugetDeps = ./deps.json; # nix build .#packages.x86_64-linux.all.passthru.fetch-deps
          dotnetFlags = [ "-p:CommitHash=${gitRev}" "-p:NativeLibraryPath=${nativeCredentials}/lib/libwabisabi.so" "-p:BuildMcwHost=false" ];
          dotnetRestoreFlags = [ "-p:Configuration=Release" ];
          dotnet-sdk = pkgs.dotnetCorePackages.sdk_10_0;
          dotnet-runtime = pkgs.dotnetCorePackages.aspnetcore_10_0;

          src = ./.;
        } // commonBuildAttrs);

        # Common build settings for all configurations
        commonBuildAttrs = rec {
          pname = "MagicalCryptoWallet";
          projectFile = [
             "MagicalCryptoWallet.Coordinator/MagicalCryptoWallet.Coordinator.csproj"
             "MagicalCryptoWallet.Tests/MagicalCryptoWallet.Tests.csproj"
             "MagicalCryptoWallet.IntegrationTests/MagicalCryptoWallet.IntegrationTests.csproj"
             "ThirdParty/WabiSabi/csharp/WabiSabi.Tests/WabiSabi.Tests.csproj"
             "ThirdParty/WabiSabi/interop/WabiSabiInterop.Tests/WabiSabiInterop.Tests.csproj"
             "MagicalCryptoWallet.Fluent.Desktop/MagicalCryptoWallet.Fluent.Desktop.csproj"];
          executables = [
            "MagicalCryptoWallet.Coordinator"
            "MagicalCryptoWallet.Fluent.Desktop" ];
          runtimeDeps = with pkgs; [
             pkgs.openssl pkgs.zlib
             # for client
             tor bitcoind
             xorg.libX11 xorg.libXrandr xorg.libX11.dev xorg.libICE xorg.libSM fontconfig.lib ];

          # Disable parallel builds to avoid Avalonia resource file locking issues
          enableParallelBuilding = false;

          # wrap manually, because we want not so ugly executable names
          dontDotnetFixup = true;

          preFixup = ''
            mkdir -p $out/bin
            cp ${mcwHost}/bin/mcw $out/lib/${pname}/mcw
            ln -s $out/lib/${pname}/mcw $out/bin/mcw
            wrapDotnetProgram $out/lib/${pname}/MagicalCryptoWallet.Fluent.Desktop $out/bin/magicalcryptowallet
            wrapDotnetProgram $out/lib/${pname}/MagicalCryptoWallet.Coordinator $out/bin/magicalcryptowallet-coordinator
            cp $out/bin/magicalcryptowallet $out/lib/${pname}/magicalcryptowallet
          '';

          binaries = "BundledApps/Binaries/linux-x64";
          bundledApps = "./MagicalCryptoWallet/${binaries}";
          bundledAppsIntegrationTest = "./MagicalCryptoWallet.IntegrationTests/${binaries}";
          preBuild = ''
            mkdir -p MagicalCryptoWallet.Fluent.Desktop/bin/Release/net10.0/linux-x64
            cp ${mcwHost}/bin/mcw MagicalCryptoWallet.Fluent.Desktop/bin/Release/net10.0/linux-x64/
            mkdir -p ${bundledApps}/Tor ${bundledAppsIntegrationTest}
            cp -r ${pkgs.tor}/bin/tor ${bundledApps}/Tor/tor
            cp ${pkgs.bitcoind}/bin/bitcoind ${bundledAppsIntegrationTest}/bitcoind
          '';
        };

        # Build everything and run unit tests (default CI target)
        buildWithUnitTests = buildMagicalCryptoWalletModule.overrideAttrs (oldAttrs: {
          doCheck = true;
          checkPhase = ''
            runHook preCheck
            dotnet MagicalCryptoWallet.Tests/bin/Release/net10.0/linux-x64/MagicalCryptoWallet.Tests.dll \
              --filter-namespace "*UnitTests*" \
              --no-progress \
              --no-ansi \
              --output Detailed
            runHook postCheck
          '';
        });

        # Build everything and run integration tests
        buildWithIntegrationTests = buildMagicalCryptoWalletModule.overrideAttrs (oldAttrs: {
          doCheck = true;
          checkPhase = ''
            runHook preCheck
            dotnet MagicalCryptoWallet.IntegrationTests/bin/Release/net10.0/linux-x64/MagicalCryptoWallet.IntegrationTests.dll \
              --no-progress \
              --no-ansi \
              --output Detailed
            runHook postCheck
          '';
        });

        # Build everything and run all tests (unit + integration)
        buildWithAllTests = buildMagicalCryptoWalletModule.overrideAttrs (oldAttrs: {
          doCheck = true;
          checkPhase = ''
            runHook preCheck
            dotnet MagicalCryptoWallet.Tests/bin/Release/net10.0/linux-x64/MagicalCryptoWallet.Tests.dll \
              --filter-namespace "*UnitTests*" \
              --no-progress \
              --no-ansi \
              --output Detailed
            dotnet MagicalCryptoWallet.IntegrationTests/bin/Release/net10.0/linux-x64/MagicalCryptoWallet.IntegrationTests.dll \
              --no-progress \
              --no-ansi \
              --output Detailed
            runHook postCheck
          '';
        });

        # dotnet trace
        dotnet-trace = pkgs.buildDotnetGlobalTool {
          pname = "dotnet-trace";
          nugetName = "dotnet-trace";
          version = "8.0.510501";
          nugetSha256 = "sha256-Kt5x8n5Q0T+BaTVufhsyjXbi/BlGKidb97DWSbI6Iq8=";
          dotnet-sdk = pkgs.dotnetCorePackages.sdk_10_0;
        };
        # dotnet dump
        dotnet-dump = pkgs.buildDotnetGlobalTool {
          pname = "dotnet-dump";
          nugetName = "dotnet-dump";
          version = "8.0.510501";
          nugetSha256 = "sha256-H7Z4EA/9G3DvVuXbnQJF7IJMEB2SkzRjTAL3eZMqCpI=";
          dotnet-sdk = pkgs.dotnetCorePackages.sdk_10_0;
        };
        # dotnet counters
        dotnet-counters = pkgs.buildDotnetGlobalTool {
          pname = "dotnet-counters";
          nugetName = "dotnet-counters";
          version = "8.0.510501";
          nugetSha256 = "sha256-gAexbRzKP/8VPhFy2OqnUCp6ze3CkcWLYR1nUqG71PI=";
          dotnet-sdk = pkgs.dotnetCorePackages.sdk_10_0;
        };
        # dotnet gcdump
        dotnet-gcdump = pkgs.buildDotnetGlobalTool {
          pname = "dotnet-gcdump";
          nugetName = "dotnet-gcdump";
          version = "8.0.510501";
          nugetSha256 = "sha256-y10InQA1sAvFYrRe+7I2+txKOvu1qQ1ii/7DnXvipxM=";
          dotnet-sdk = pkgs.dotnetCorePackages.sdk_10_0;
        };

        magicalcryptowallet-shell =
          with {
            libs = with pkgs; [
              xorg.libX11
              xorg.libXrandr
              xorg.libX11.dev
              xorg.libICE
              xorg.libSM
              pkgs.zlib
              fontconfig.lib
            ];
            skiaSharp=toString ./. + "/MagicalCryptoWallet.Fluent.Desktop/bin/Debug/net10.0/runtimes/linux-x64/native";
          };
          pkgs.mkShell {
            name = "magicalcryptowallet-shell";
            buildInputs = libs;
            packages = [
              pkgs.dotnetCorePackages.sdk_10_0
              pkgs.cmake
              pkgs.gcc

              # tools
              dotnet-trace
              dotnet-dump
              dotnet-gcdump
              dotnet-counters

              # dependencies
              pkgs.bitcoind
              pkgs.tor

              # IDE
              pkgsUnfree.jetbrains.rider

              # Claude code
              pkgsUnfree.claude-code
              pkgs.python314 # claude loves python
           ];

            DOTNET_CLI_TELEMETRY_OPTOUT = 1;
            AVALONIA_TELEMETRY_OPTOUT=1;
            DOTNET_NOLOGO = 1;
            DOTNET_ROOT = "${pkgs.dotnetCorePackages.sdk_10_0}";
            DOTNET_GLOBAL_TOOLS_PATH = "${builtins.getEnv "HOME"}/.dotnet/tools";
            #DOTNET_ROLL_FORWARD = "latestPatch";
            LD_LIBRARY_PATH = "${skiaSharp};${pkgs.lib.makeLibraryPath libs}";
            BUNDLED_APPS_BINARIES_PATH = "MagicalCryptoWallet/BundledApps/Binaries/linux-x64";
            BUNDLED_APPS_INTEGRATION_TEST_BINARIES_PATH = "MagicalCryptoWallet.IntegrationTests/BundledApps/Binaries/linux-x64";

            shellHook = ''
              export PATH="$PATH:$DOTNET_GLOBAL_TOOLS_PATH"
              cp $(which tor) "$BUNDLED_APPS_BINARIES_PATH/Tor/"
              cp $(which bitcoind) "$BUNDLED_APPS_INTEGRATION_TEST_BINARIES_PATH/"

              export PS1='\n\[\033[1;34m\][MagicalCryptoWallet:\w]\$\[\033[0m\] '
            '';
        };
    in
    {
      packages.x86_64-linux = {
        default = buildWithUnitTests;
        unit-tests = buildWithUnitTests;
        integration-tests = buildWithIntegrationTests;
        all = buildWithAllTests;
      };
      devShells.x86_64-linux.default = magicalcryptowallet-shell;
    };
}
