//! M4 reference application as a library, so end-to-end tests can start a real
//! server on a real socket with a real SQLite file.

pub mod http;
pub mod routes;

pub use http::{read_request, Response};
pub use routes::{run, Running, ServerConfig};
