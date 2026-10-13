//! Command-line grammar (spec-narrate-cli.md, Grammar).

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

use crate::generate::Options;

const VERSION: &str = if crate::hooks::ENABLED {
    concat!(env!("CARGO_PKG_VERSION"), " (test hooks enabled)")
} else {
    env!("CARGO_PKG_VERSION")
};

/// Turn a Markdown or text script into a 24 kHz mono WAV file with Chatterbox-Turbo.
#[derive(Parser, Debug)]
#[command(
    name = "narrate",
    version = VERSION,
    subcommand_negates_reqs = true,
    after_help = "Run `narrate setup` once to install the Python environment and download the models."
)]
pub struct Cli {
    /// Print worker and uv output, chunk text, and error details.
    #[arg(long, global = true)]
    pub verbose: bool,

    #[command(subcommand)]
    pub command: Option<Command>,

    #[command(flatten)]
    pub generate: GenerateArgs,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Install the Python environment, models, and voice clips (needs network, once).
    Setup,
    /// Report whether setup is complete for this binary.
    Status,
}

#[derive(Args, Debug)]
pub struct GenerateArgs {
    /// Script to narrate: a .md, .markdown, or .txt file.
    #[arg(value_name = "FILE", required = true)]
    pub file: Option<PathBuf>,

    /// Output WAV path [default: FILE with the extension replaced by .wav].
    #[arg(short, long, value_name = "PATH")]
    pub output: Option<PathBuf>,

    /// Voice preset [default: female].
    #[arg(long, value_enum, conflicts_with = "voice_file")]
    pub voice: Option<VoicePreset>,

    /// Reference clip (WAV) to clone instead of a preset.
    #[arg(long, value_name = "WAV")]
    pub voice_file: Option<PathBuf>,

    /// Where to run the model.
    #[arg(long, value_enum)]
    pub device: Option<Device>,
}

#[derive(ValueEnum, Clone, Copy, Debug)]
pub enum VoicePreset {
    Female,
    Male,
    Builtin,
}

#[derive(ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Device {
    Auto,
    Cuda,
    Cpu,
}

/// The voice a generation run uses.
#[derive(Clone, Debug)]
pub enum VoiceChoice {
    Female,
    Male,
    Builtin,
    File(PathBuf),
}

impl GenerateArgs {
    /// Build the run options; `None` when no script was given (a subcommand ran instead).
    pub fn into_options(self) -> Option<Options> {
        let file = self.file?;
        let voice = match (self.voice_file, self.voice) {
            (Some(path), _) => VoiceChoice::File(path),
            (None, Some(VoicePreset::Male)) => VoiceChoice::Male,
            (None, Some(VoicePreset::Builtin)) => VoiceChoice::Builtin,
            (None, Some(VoicePreset::Female) | None) => VoiceChoice::Female,
        };
        Some(Options {
            file,
            output: self.output,
            voice,
            device: self.device.unwrap_or(Device::Auto),
        })
    }
}

impl GenerateArgs {
    /// True when any generation argument was given (they are not allowed with a subcommand).
    pub fn any_given(&self) -> bool {
        self.file.is_some()
            || self.output.is_some()
            || self.voice.is_some()
            || self.voice_file.is_some()
            || self.device.is_some()
    }
}
