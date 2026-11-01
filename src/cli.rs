use std::ffi::OsString;
use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

#[derive(Parser)]
#[command(name = "maxim-native", version, about = "Offline Maxim text-to-speech")]
pub(crate) struct Cli {
    #[arg(
        long,
        global = true,
        default_value = "extracted/app/vox/vox_ru_maxim22i",
        help = "Path to the local Maxim VOX database"
    )]
    pub(crate) voice: PathBuf,

    #[command(subcommand)]
    pub(crate) command: Command,
}

#[derive(Clone, Copy, Default, Eq, PartialEq, ValueEnum)]
pub(crate) enum JoinChoice {
    #[default]
    Original,
    Basic,
    Acoustic,
}

#[derive(Subcommand)]
pub(crate) enum Command {
    /// Convert text to a WAV file (the default command)
    Speak {
        /// Text to speak; use - to read standard input
        text: String,
        #[arg(short, long, default_value = "output.wav")]
        output: PathBuf,
        #[arg(long, value_enum, default_value_t = JoinChoice::Original)]
        join: JoinChoice,
        #[arg(long)]
        simple_phones: bool,
        #[arg(long)]
        heuristic_target: bool,
        /// Show phones, unit choices, and sample counts; disables the progress bar
        #[arg(short, long)]
        verbose: bool,
        /// Play the generated WAV with a local audio player
        #[arg(long)]
        play: bool,
        /// Selection workers (0 chooses automatically; 1 disables parallel selection)
        #[arg(long, default_value_t = 0)]
        jobs: usize,
    },
    /// Show VOX metadata
    Inspect,
    /// Precompute exact unit validity and coverage for faster synthesis
    BuildUnitIndex {
        #[arg(long, default_value_t = 8)]
        jobs: usize,
    },
    /// List corpus candidate IDs for a diphone
    Diphone { name: String },
    /// Show original pronunciation phones for text
    Phones { text: String },
    /// Show original syllable boundaries for text
    Syllables { text: String },
    /// Show the earlier simple phone mapping
    PhonesSimple { text: String },
    /// Show expanded candidate metadata
    Features { unit_id: u32 },
    /// Show the duration model estimate for one phone
    DurationModel { phone: String },
    /// Show duration estimates for text
    DurationText { text: String },
    /// Decode one SILK packet to WAV
    DecodePacket {
        offset: String,
        #[arg(short, long, default_value = "packet.wav")]
        output: PathBuf,
    },
    /// Decode one corpus unit to WAV
    DecodeUnit {
        index: u32,
        #[arg(short, long, default_value = "unit.wav")]
        output: PathBuf,
    },
    /// Dump all decodable candidates for a diphone
    DumpDiphone {
        name: String,
        #[arg(short, long, default_value = "diphone.mdu")]
        output: PathBuf,
    },
    /// Look up a word in the original Russian lexicon
    Lookup { word: String, pos: Option<char> },
    /// Pronounce a word with lexicon and original LTS fallback
    Pronounce { word: String, pos: Option<char> },
}

pub(crate) fn parse() -> Cli {
    let mut args: Vec<OsString> = std::env::args_os().collect();
    if should_inject_speak(&args) {
        args.insert(1, OsString::from("speak"));
    }
    Cli::parse_from(args)
}

fn should_inject_speak(args: &[OsString]) -> bool {
    let known = [
        "speak",
        "inspect",
        "build-unit-index",
        "diphone",
        "phones",
        "syllables",
        "phones-simple",
        "features",
        "duration-model",
        "duration-text",
        "decode-packet",
        "decode-unit",
        "dump-diphone",
        "lookup",
        "pronounce",
        "help",
    ];
    let mut index = 1;
    while let Some(arg) = args.get(index).and_then(|arg| arg.to_str()) {
        if matches!(arg, "--help" | "-h" | "--version" | "-V") {
            return false;
        }
        if matches!(arg, "--voice" | "-o" | "--output" | "--join") {
            index += 2;
            continue;
        }
        if arg == "-" {
            return true;
        }
        if arg.starts_with('-') {
            index += 1;
            continue;
        }
        return !known.contains(&arg);
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_command_respects_global_options_and_diagnostics() {
        let args = |items: &[&str]| items.iter().map(OsString::from).collect::<Vec<_>>();
        assert!(should_inject_speak(&args(&["maxim-native", "Привет!"])));
        assert!(should_inject_speak(&args(&[
            "maxim-native",
            "--voice",
            "/tmp/voice",
            "Привет!"
        ])));
        assert!(!should_inject_speak(&args(&[
            "maxim-native",
            "--voice",
            "/tmp/voice",
            "inspect"
        ])));
        assert!(!should_inject_speak(&args(&[
            "maxim-native",
            "help",
            "speak"
        ])));
        assert!(should_inject_speak(&args(&[
            "maxim-native",
            "-",
            "-o",
            "papa.wav"
        ])));
    }
}
