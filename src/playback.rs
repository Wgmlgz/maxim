use std::error::Error;
use std::io;
use std::path::Path;
use std::process::Command;

pub(crate) fn play(path: &Path) -> Result<(), Box<dyn Error>> {
    let absolute = path.canonicalize()?;
    #[cfg(target_os = "macos")]
    let players: &[(&str, &[&str])] = &[("afplay", &[])];
    #[cfg(target_os = "linux")]
    let players: &[(&str, &[&str])] = &[
        ("pw-play", &[]),
        ("paplay", &[]),
        ("aplay", &[]),
        ("ffplay", &["-nodisp", "-autoexit", "-loglevel", "error"]),
    ];
    #[cfg(target_os = "windows")]
    let players: &[(&str, &[&str])] = &[(
        "powershell.exe",
        &[
            "-NoProfile",
            "-Command",
            "(New-Object System.Media.SoundPlayer $args[0]).PlaySync();",
        ],
    )];
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    let players: &[(&str, &[&str])] = &[];

    let mut failures = Vec::new();
    for &(program, options) in players {
        match Command::new(program).args(options).arg(&absolute).status() {
            Ok(status) if status.success() => return Ok(()),
            Ok(status) => failures.push(format!("{program}: {status}")),
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => failures.push(format!("{program}: {error}")),
        }
    }
    if failures.is_empty() {
        return Err(
            "no local WAV player found (install PipeWire, PulseAudio, ALSA, or ffplay)".into(),
        );
    }
    Err(format!(
        "could not play {}: {}",
        absolute.display(),
        failures.join(", ")
    )
    .into())
}
