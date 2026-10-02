#![forbid(unsafe_code)]
use crate::{
    app, platform,
    qr::{self, Ecc},
};
use std::{
    ffi::OsString,
    io::{self, Read, Write},
};

pub const HELP: &str = "Magical Crypto Wallet\n\nUsage: mcw [gui] [application arguments]\n       mcw qr encode [--ecc L|M|Q|H]\n       mcw --help | --version\n\nQR text is read verbatim from UTF-8 stdin. Output is the width followed by\nrows of 0/1 modules, without a quiet zone. Correction defaults to M.\nThe desktop currently hosts the managed application; mcw owns its lifetime.\n";

pub fn run(mut args: Vec<OsString>) -> Result<i32, String> {
    match args.first().and_then(|arg| arg.to_str()) {
        Some("--help" | "-h") => {
            platform::attach_console();
            print!("{HELP}");
            Ok(0)
        }
        Some("--version" | "-V") => {
            platform::attach_console();
            println!("mcw {}", crate::VERSION);
            Ok(0)
        }
        Some("qr") => {
            platform::attach_console();
            let ecc = qr_args(&args[1..])?;
            let mut input = Vec::new();
            io::stdin()
                .take(1_048_577)
                .read_to_end(&mut input)
                .map_err(|_| "cannot read stdin")?;
            let text = std::str::from_utf8(&input).map_err(|_| "stdin is not valid UTF-8")?;
            let symbol = qr::encode(text, ecc).map_err(str::to_owned)?;
            let mut out = io::BufWriter::new(io::stdout().lock());
            writeln!(out, "{}", symbol.width).map_err(|_| "cannot write stdout")?;
            for row in symbol.modules.chunks(symbol.width) {
                for module in row {
                    out.write_all(if *module == 1 { b"1" } else { b"0" })
                        .map_err(|_| "cannot write stdout")?;
                }
                out.write_all(b"\n").map_err(|_| "cannot write stdout")?;
            }
            out.flush().map_err(|_| "cannot write stdout")?;
            Ok(0)
        }
        Some("gui") => {
            args.remove(0);
            app::run(args)
        }
        Some(value)
            if !value.starts_with('-') && value != "startsilent" && value != "crashreport" =>
        {
            Err("Unknown application command. Use --help for supported options.".into())
        }
        _ => app::run(args),
    }
}

fn qr_args(args: &[OsString]) -> Result<Ecc, String> {
    if args.first().and_then(|arg| arg.to_str()) != Some("encode") {
        return Err("expected qr encode [--ecc L|M|Q|H]".into());
    }
    match &args[1..] {
        [] => Ok(Ecc::M),
        [flag, value] if flag == "--ecc" => value
            .to_str()
            .and_then(Ecc::parse)
            .ok_or("correction level must be L, M, Q or H".into()),
        _ => Err("expected qr encode [--ecc L|M|Q|H]".into()),
    }
}
