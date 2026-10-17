//! Optional polyphonic motion. Immutable programs are shared; clocks belong to notes.
use alloc::{string::String, vec::Vec};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Program {
    pub lanes: Vec<Lane>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Lane {
    pub index: usize,
    pub id: String,
    pub parameter: usize,
    pub amount: f64,
    pub beats: f64,
    pub start: usize,
    pub end: usize,
    pub direction: u8,
    pub one_shot: bool,
    pub slide: bool,
    pub values: Vec<f64>,
    pub durations: Vec<f64>,
    pub probabilities: Vec<f64>,
    pub curves: Vec<u8>,
}
