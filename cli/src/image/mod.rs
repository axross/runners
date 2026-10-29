//! the runner image as the CLI knows it: the embedded definition, the contract it
//! declares, the build context made from it, and the check of a built image's
//! labels; see `docs/operations/cli.md`.

pub mod archive;
pub mod check;
pub mod contract;
pub mod definition;
#[cfg(test)]
mod dockerfile;
