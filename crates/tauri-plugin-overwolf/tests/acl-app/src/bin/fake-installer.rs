//! A stand-in update installer for the update tests: writes its arguments,
//! one per line, to `<own path>.args` (whole, by a rename) and exits.

use std::io::Write as _;

fn main() -> std::io::Result<()> {
    let me = std::env::current_exe()?;
    let part = me.with_extension("args.part");
    let mut out = std::fs::File::create(&part)?;
    for arg in std::env::args().skip(1) {
        writeln!(out, "{arg}")?;
    }
    out.sync_all()?;
    drop(out);
    std::fs::rename(part, me.with_extension("args"))
}
