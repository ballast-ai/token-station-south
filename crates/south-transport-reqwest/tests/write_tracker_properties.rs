//! Test the same parser bytes that the optional HTTP patch installs.
#[path = "../../../patches/reqwest-0.13.5-local-write/parser.rs"]
mod parser;
use parser::WriteTracker;
include!("../../../patches/reqwest-0.13.5-local-write/property_cases.rs");
