use std::path::Path;
use std::process::Command;

use crate::pages::Error;

pub fn command() -> Result<Command, Error> {
    let editor = std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .map_err(|_| "Set VISUAL or EDITOR to a waiting editor, e.g. 'code --wait' or 'vi'")?;
    let args = shell_words::split(&editor)?;
    let (program, args) = args
        .split_first()
        .ok_or("The editor command must not be empty")?;
    let mut command = Command::new(program);
    command.args(args);
    Ok(command)
}

pub fn run(path: &Path) -> Result<(), Error> {
    let status = command()?.arg(path).status()?;
    if !status.success() {
        return Err(format!("Editor exited with {status}; no changes saved to SQL").into());
    }
    Ok(())
}
