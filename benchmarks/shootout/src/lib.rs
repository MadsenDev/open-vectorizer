//! Shared pieces of the comparison harness.
//!
//! Node extraction lives here rather than inside `score` because the README
//! figure draws a marker on every node it finds. If the figure and the tables
//! used different parsers, a reader could count the dots in the picture and get
//! a different answer from the table underneath it.

pub mod nodes;
