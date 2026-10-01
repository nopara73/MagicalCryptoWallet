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
        buildMagicalCryptoWalletModule = pkgs.buildDotnetModule {
          pname = "magicalcryptowallet";
          version = "2.0.0-${builtins.substring 0 8 (self.lastModifiedDate or self.lastModified or "19700101")}-${gitRev}";
          nugetDeps = ./deps.json; # nix build .#packages.x86_64-linux.all.passthru.fetch-deps
          dotnetFlags = [ "-p:CommitHash=${gitRev}" "-p:NativeLibraryPath=${nativeCredentials}/lib/libwabisabi.so" ];
          dotnet-sdk = pkgs.dotnetCorePackages.sdk_10_0;
          dotnet-runtime = pkgs.dotnetCorePackages.aspnetcore_10_0;

          src = ./.;
        };

        # Common build settings for all configurations
        commonBuildAttrs = rec {
          pname = "MagicalCryptoWallet";
          projectFile = [
             "MagicalCryptoWallet.Coordinator/MagicalCryptoWallet.Coordinator.csproj"
             "MagicalCryptoWallet.Daemon/MagicalCryptoWallet.Daemon.csproj"
             "MagicalCryptoWallet.Tests/MagicalCryptoWallet.Tests.csproj"
             "MagicalCryptoWallet.IntegrationTests/MagicalCryptoWallet.IntegrationTests.csproj"
             "ThirdParty/WabiSabi/csharp/WabiSabi.Tests/WabiSabi.Tests.csproj"
             "ThirdParty/WabiSabi/interop/WabiSabiInterop.Tests/WabiSabiInterop.Tests.csproj"
             "MagicalCryptoWallet.Fluent.Desktop/MagicalCryptoWallet.Fluent.Desktop.csproj"];
          executables = [
            "MagicalCryptoWallet.Coordinator"
            "MagicalCryptoWallet.Daemon"
            "MagicalCryptoWallet.Fluent.Desktop" ];
          runtimeDeps = with pkgs; [
             pkgs.openssl pkgs.zlib
             # for client
             tor hwi bitcoind
             xorg.libX11 xorg.libXrandr xorg.libX11.dev xorg.libICE xorg.libSM fontconfig.lib ];

          # Disable parallel builds to avoid Avalonia resource file locking issues
          enableParallelBuilding = false;

          # wrap manually, because we want not so ugly executable names
          dontDotnetFixup = true;

          preFixup = ''
            wrapDotnetProgram $out/lib/${pname}/MagicalCryptoWallet.Fluent.Desktop $out/bin/magicalcryptowallet
            wrapDotnetProgram $out/lib/${pname}/MagicalCryptoWallet.Coordinator $out/bin/magicalcryptowallet-coordinator
            wrapDotnetProgram $out/lib/${pname}/MagicalCryptoWallet.Daemon $out/bin/magicalcryptowalletd
          '';

          binaries = "BundledApps/Binaries/linux-x64";
          bundledApps = "./MagicalCryptoWallet/${binaries}";
          bundledAppsIntegrationTest = "./MagicalCryptoWallet.IntegrationTests/${binaries}";
          preBuild = ''
            cp -r ${pkgs.tor}/bin/tor ${bundledApps}/Tor/tor
            cp ${pkgs.hwi}/bin/hwi ${bundledApps}/hwi
            cp ${pkgs.bitcoind}/bin/bitcoind ${bundledAppsIntegrationTest}/bitcoind
          '';
        };

        # Build everything and run unit tests (default CI target)
        buildWithUnitTests = buildMagicalCryptoWalletModule.overrideAttrs (oldAttrs: commonBuildAttrs // {
          doCheck = true;
          checkPhase = ''
            runHook preCheck
            dotnet test --project MagicalCryptoWallet.Tests/MagicalCryptoWallet.Tests.csproj \
              --no-build \
              --configuration Release \
              --filter-namespace "*UnitTests*" \
              --no-progress \
              --no-ansi \
              --output Detailed
            runHook postCheck
          '';
        });

        # Build everything and run integration tests
        buildWithIntegrationTests = buildMagicalCryptoWalletModule.overrideAttrs (oldAttrs: commonBuildAttrs // {
          doCheck = true;
          checkPhase = ''
            runHook preCheck
            dotnet test --project MagicalCryptoWallet.IntegrationTests/MagicalCryptoWallet.IntegrationTests.csproj \
              --no-build \
              --configuration Release \
              --no-progress \
              --no-ansi \
              --output Detailed
            runHook postCheck
          '';
        });

        # Build everything and run all tests (unit + integration)
        buildWithAllTests = buildMagicalCryptoWalletModule.overrideAttrs (oldAttrs: commonBuildAttrs // {
          doCheck = true;
          checkPhase = ''
            runHook preCheck
            dotnet test --project MagicalCryptoWallet.Tests/MagicalCryptoWallet.Tests.csproj \
              --filter-namespace "*UnitTests*" \
              --no-build \
              --configuration Release \
              --no-progress \
              --no-ansi \
              --output Detailed
            dotnet test --project MagicalCryptoWallet.IntegrationTests/MagicalCryptoWallet.IntegrationTests.csproj \
              --no-build \
              --configuration Release \
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
              pkgs.hwi

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
              cp $(which hwi) "$BUNDLED_APPS_BINARIES_PATH/hwi"
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
