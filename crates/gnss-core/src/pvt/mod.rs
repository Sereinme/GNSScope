//! PVT (Position, Velocity, Time) solving.
//!
//! Pipeline: ephemeris parsing → satellite position → atmospheric corrections →
//! pseudorange → WLS solver → ECEF/LLA coordinates.

pub mod atmosphere;
pub mod coords;
pub mod pseudorange;
pub mod satpos;
pub mod solver;

pub use coords::{ecef_to_lla, lla_to_ecef, Ecef, Lla};
pub use pseudorange::Pseudorange;
pub use satpos::{satellite_position, SatPos};
pub use solver::{solve_wls, PvtSolution};
