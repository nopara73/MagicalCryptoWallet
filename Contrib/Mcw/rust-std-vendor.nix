# Compiler standard-library inputs only; mcw has no external Cargo dependencies.
{ pkgs }: let
  packages = [
    { name = "addr2line"; version = "0.27.1"; sha256 = "e567177890eb1617b1f774005b66b26b2377afd138a2ca37aae7d8f0c81429d4"; }
    { name = "adler2"; version = "2.0.1"; sha256 = "320119579fcad9c21884f5c4861d16174d0e06250625266f50fe6898340abefa"; }
    { name = "cc"; version = "1.2.0"; sha256 = "1aeb932158bd710538c73702db6945cb68a8fb08c519e6e12706b94263b36db8"; }
    { name = "cfg-if"; version = "1.0.4"; sha256 = "9330f8b2ff13f34540b44e946ef35111825727b38d33286ef986142615121801"; }
    { name = "dlmalloc"; version = "0.2.14"; sha256 = "ad5208a115eaba24916f7456929832e310a81518c641f93fee4f89aa93aa3675"; }
    { name = "foldhash"; version = "0.2.0"; sha256 = "77ce24cb58228fbb8aa041425bb1050850ac19177686ea6e0f41a70416f56fdb"; }
    { name = "fortanix-sgx-abi"; version = "0.6.1"; sha256 = "5efc85edd5b83e8394f4371dd0da6859dff63dd387dab8568fece6af4cde6f84"; }
    { name = "getopts"; version = "0.2.24"; sha256 = "cfe4fbac503b8d1f88e6676011885f34b7174f46e59956bba534ba83abded4df"; }
    { name = "gimli"; version = "0.34.0"; sha256 = "1033caf0b349c518623b5396bfb2cf0bddf44f0306d543a250e5743297aafd10"; }
    { name = "hashbrown"; version = "0.17.1"; sha256 = "ed5909b6e89a2db4456e54cd5f673791d7eca6732202bbf2a9cc504fe2f9b84a"; }
    { name = "hermit-abi"; version = "0.5.2"; sha256 = "fc0fef456e4baa96da950455cd02c081ca953b141298e41db3fc7e36b1da849c"; }
    { name = "libc"; version = "0.2.189"; sha256 = "3eaf3ede3fee6db1a4c2ee091bf8a8b4dccdc6d17f656fb07896ee72867612f2"; }
    { name = "memchr"; version = "2.8.3"; sha256 = "cf8baf1c55e62ffcace7a9f06f4bd9cd3f0c4beb022d3b367256b91b87513d98"; }
    { name = "miniz_oxide"; version = "0.9.1"; sha256 = "b63fbc4a50860e98e7b2aa7804ded1db5cbc3aff9193adaff57a6931bf7c4b4c"; }
    { name = "moto-rt"; version = "0.16.4"; sha256 = "0aadbab5a5ca5a01ec1ff4bc03c2c3a7643f0e1d97ad5a4a5b58ce78504e17da"; }
    { name = "object"; version = "0.39.1"; sha256 = "2e5a6c098c7a3b6547378093f5cc30bc54fd361ce711e05293a5cc589562739b"; }
    { name = "r-efi"; version = "5.3.0"; sha256 = "69cdb34c158ceb288df11e18b4bd39de994f6657d83847bdffdbd7f346754b0f"; }
    { name = "r-efi-alloc"; version = "2.1.0"; sha256 = "dc2f58ef3ca9bb0f9c44d9aa8537601bcd3df94cc9314a40178cadf7d4466354"; }
    { name = "rand"; version = "0.9.5"; sha256 = "b9ef1d0d795eb7d84685bca4f72f3649f064e6641543d3a8c415898726a57b41"; }
    { name = "rand_core"; version = "0.9.5"; sha256 = "76afc826de14238e6e8c374ddcc1fa19e374fd8dd986b0d2af0d02377261d83c"; }
    { name = "rand_xorshift"; version = "0.4.0"; sha256 = "513962919efc330f829edb2535844d1b912b0fbe2ca165d613e4e8788bb05a5a"; }
    { name = "rustc-demangle"; version = "0.1.28"; sha256 = "b74b56ffa8bb2830709a538c2cbcae9aa062db0d2a42563bfb09bdaae44020eb"; }
    { name = "rustc-literal-escaper"; version = "0.0.8"; sha256 = "bfe6f213fb658c8fb95baabd5420393438cf5a98d707f5dd701d9197c705f71e"; }
    { name = "shlex"; version = "1.3.0"; sha256 = "0fda2ff0d084019ba4d7c6f371c95d8fd75ce3524c3cb8fb653a3023f6323e64"; }
    { name = "unwinding"; version = "0.2.10"; sha256 = "4b134ada16dda9e435abe2a6d76a01d497bc60707357845a15f9b0ed42dc88ce"; }
    { name = "vex-sdk"; version = "0.27.1"; sha256 = "79e5fe15afde1305478b35e2cb717fff59f485428534cf49cfdbfa4723379bf6"; }
    { name = "wasip1"; version = "1.0.0"; sha256 = "b5e26842486624357dbeb8f0381cf1fb42f022291fd787d4a816768fec8cc760"; }
    { name = "wasip2"; version = "1.0.4+wasi-0.2.12"; sha256 = "b67efb37e106e55ce722a510d6b5f9c17f083e5fc79afc2badeb12cc313d9487"; }
    { name = "wasip3"; version = "0.7.0+wasi-0.3.0"; sha256 = "07aa681120704bf09828d1f7cf7e763666c1dc6a36e5096efd0ad9aadfb302d3"; }
    { name = "wit-bindgen"; version = "0.57.1"; sha256 = "1ebf944e87a7c253233ad6766e082e3cd714b5d03812acc24c318f549614536e"; }
  ];
  archives = map (p: pkgs.fetchurl {
    url = "https://static.crates.io/crates/${p.name}/${p.name}-${p.version}.crate";
    sha256 = p.sha256;
  }) packages;
in pkgs.stdenvNoCC.mkDerivation {
  pname = "mcw-rust-1.99-std-vendor";
  version = "1";
  srcs = archives;
  sourceRoot = ".";
  unpackPhase = ''
    for archive in $srcs; do tar -xzf "$archive"; done
  '';
  dontConfigure = true;
  dontBuild = true;
  installPhase = ''
    mkdir -p $out
    cp -R ./* $out/
    ${pkgs.lib.concatMapStringsSep "\n" (p: "cp ${pkgs.writeText "checksum.json" (builtins.toJSON { package = p.sha256; files = {}; })} $out/${p.name}-${p.version}/.cargo-checksum.json") packages}
  '';
}
