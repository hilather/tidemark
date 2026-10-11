//! Owned by: W4-B
//! Deployment command exit-code tests and compiled VM scenarios.

#[path = "w4b/mod.rs"]
mod w4b;

// Compile the deployment boundary and its fake regressions independently of unrelated
// command unit tests whose Context constructors require coordinator updates.
#[allow(dead_code, unused_imports)]
#[path = "../src/deploy/mod.rs"]
mod deploy;
use tidemark_cli::{client, CliError, Context, ACCESS_HINT};
#[allow(dead_code)]
#[path = "../src/cmd/deploy/render.rs"]
mod render;
#[allow(dead_code)]
#[path = "../src/cmd/deploy/stage.rs"]
mod stage_adapter;
