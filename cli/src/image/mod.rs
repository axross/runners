//! The runner image as the CLI knows it: the definition embedded in the binary, the
//! contract it declares, the build context made from it, and the check of a built
//! image's labels.
//!
//! `build` sends the embedded definition to Docker as an archive and reads nothing
//! from a checkout. `start` and each job launch check the configured image's
//! contract label against the one this binary embeds, and `start` and `status`
//! report the image's freshness.

pub mod archive;
pub mod check;
pub mod contract;
pub mod definition;
#[cfg(test)]
mod dockerfile;
