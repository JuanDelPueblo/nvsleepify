use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug)]
pub enum Command {
    Status,
    Sleep { kill_procs: bool },
    Wake,
}

#[derive(Serialize, Deserialize, Debug)]
pub enum Response {
    Ok,
    Error(String),
    StatusOutput(String),
    ProcessesRunning(Vec<(String, String)>),
}
