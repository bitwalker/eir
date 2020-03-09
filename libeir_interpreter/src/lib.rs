//#![deny(warnings)]

//! LIR interpreter with zero consideration of performance.
//! Made as an experiment to narrow down relevant implementation
//! details.

mod term;
pub use term::{ TermType, Term, Pid, Reference, ErlEq, ErlExactEq, ErlOrd };

pub mod erl_lib;

mod vm;
pub use vm::{ VMState, WatchType };

mod process;

mod module;

//mod trace;
