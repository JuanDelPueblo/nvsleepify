use clap::ValueEnum;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Default)]
pub enum Mode {
    #[default]
    Standard, // nvsleepify off (GPU awake)
    Integrated, // nvsleepify on (GPU asleep)
    Optimized,  // nvsleepify auto
    Vfio,       // GPU bound to vfio-pci
}

impl std::fmt::Display for Mode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Mode::Standard => write!(f, "Standard"),
            Mode::Integrated => write!(f, "Integrated"),
            Mode::Optimized => write!(f, "Optimized"),
            Mode::Vfio => write!(f, "Vfio"),
        }
    }
}

impl std::str::FromStr for Mode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "standard" | "std" | "off" => Ok(Mode::Standard),
            "integrated" | "int" | "on" => Ok(Mode::Integrated),
            "optimized" | "opt" | "auto" => Ok(Mode::Optimized),
            "vfio" => Ok(Mode::Vfio),
            _ => Err(format!("Unknown mode: {}", s)),
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub enum Command {
    Status,
    Set(Mode),
    Delay(u32),
}

#[derive(Serialize, Deserialize, Debug)]
pub enum Response {
    Ok,
    Error(String),
    StatusOutput(String),
    ProcessesRunning(Vec<(String, String)>),
}
