//! Versioned, bounded local control protocol for Opit Speech to Text.
//! Payloads deliberately have no Debug implementation: responses can contain transcripts.

mod framing;
mod protocol;
pub mod transport;

pub use framing::{read_frame, write_frame};
pub use protocol::{
    Command, Error, Format, MAX_REQUEST, MAX_RESPONSE, Outcome, Request, Response, VERSION, decode_request,
};

#[cfg(windows)]
pub use transport::windows;
