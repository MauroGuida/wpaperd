use std::path::PathBuf;

use clap::Parser;
use serde::{Deserialize, Serialize};

#[derive(Default, Parser, Serialize, Deserialize)]
#[clap(
    author = "Danilo Spinella <danilo.spinella@suse.com>",
    version,
    about = "A wallpaper manager for Wayland compositors"
)]
pub struct Config {
    #[clap(
        action,
        short,
        long,
        help = "Path to the configuration (XDG_CONFIG_HOME/wpaperd/wpaperd.toml by default)"
    )]
    #[serde(skip)]
    pub config: Option<PathBuf>,
    #[clap(
        action,
        short,
        long = "wallpaper-config",
        help = "Path to the configuration of the wallpaper (XDG_CONFIG_HOME/wpaperd/wallpaper.toml by default)"
    )]
    pub wallpaper_config: Option<PathBuf>,
    #[clap(
        action,
        short = 'n',
        long = "no-daemon",
        help = "Stay in foreground, do not detach"
    )]
    #[serde(rename = "no-daemon")]
    pub no_daemon: bool,
    #[clap(short, long, help = "Increase the verbosity of wpaperd")]
    pub verbose: bool,
    #[clap(
        long,
        help = "Fd to write once wpaperd is running (used for readiness)"
    )]
    pub notify: Option<u8>,
}
