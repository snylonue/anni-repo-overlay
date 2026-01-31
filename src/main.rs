use std::{
    fs::{self, File},
    io::Read,
    ops::Not,
    path::{Path, PathBuf},
    process::Command,
};

use anni_repo::RepositoryManager;
use anyhow::anyhow;
use clap::Parser;
use serde::Deserialize;

#[derive(Debug, Parser)]
#[command(version, about, long_about)]
struct App {
    #[clap(short = 'c')]
    config: PathBuf,

    output_directory: PathBuf,
}

#[derive(Debug, Clone, Deserialize)]
struct Config {
    base: Repo,
    overlay: Vec<Repo>,
}

#[derive(Debug, Clone, Deserialize)]
struct Repo {
    name: String,
    url: String,
}

fn git_clone(url: &str, to: &str) -> anyhow::Result<()> {
    log::info!("cloning {url} to {to}");
    let cmd = Command::new("git").args(["clone", url, to]).output()?;
    if cmd.status.success() {
        Ok(())
    } else {
        Err(anyhow!(
            "Failed to clone {url}: {}",
            String::from_utf8_lossy(&cmd.stderr)
        ))
    }
}

// returns true if cloned
fn git_clone_if_nonexist(url: &str, to: &str) -> anyhow::Result<bool> {
    if !fs::exists(to)? {
        git_clone(url, to).map(|_| true)
    } else {
        Ok(false)
    }
}

// returns true if alread up-to-date
fn git_pull(root: &str) -> anyhow::Result<bool> {
    log::info!("pulling {root}");
    let cmd = Command::new("git")
        .args(["pull", "--ff-only"])
        .current_dir(root)
        .output()?;

    if cmd.status.success() {
        log::info!("{root} already up to date");
        Ok(String::from_utf8_lossy(&cmd.stdout).contains("Already up to date."))
    } else {
        Err(anyhow!("fail to pull repo at {root}"))
    }
}

fn anni_overlay<'a>(
    base: &str,
    overlays: impl Iterator<Item = &'a str>,
    to: &Path,
) -> anyhow::Result<()> {
    log::info!("generating database");
    let base_repo = RepositoryManager::new(base)?.into_owned_manager()?;
    let overlays_repo = overlays
        .map(|overlay| RepositoryManager::new(overlay).and_then(|repo| repo.into_owned_manager()))
        .collect::<Result<Vec<_>, _>>()?;

    base_repo.apply_overlay(overlays_repo, to.join("repo.db"))?;
    Ok(())
}

fn main() -> anyhow::Result<()> {
    env_logger::init();

    let app = App::parse();
    let config = {
        let mut file = File::open(app.config)?;
        let mut buf = String::new();
        file.read_to_string(&mut buf)?;
        buf
    };
    let config: Config = toml::from_str(&config)?;

    let clone_and_pull = |url, name| {
        git_clone_if_nonexist(url, name).and_then(|cloned| {
            if !cloned {
                git_pull(name).map(Not::not)
            } else {
                Ok(true)
            }
        })
    };

    let is_fresh = clone_and_pull(&config.base.url, &config.base.name)?;
    let is_fresh = config
        .overlay
        .iter()
        .try_fold(is_fresh, |fresh, Repo { name, url }| {
            clone_and_pull(url, name).map(|f| fresh || f)
        })?;

    if is_fresh
        || !fs::exists(app.output_directory.join("repo.db"))?
        || !fs::exists(app.output_directory.join("repo.json"))?
    {
        anni_overlay(
            &config.base.name,
            config.overlay.iter().map(|Repo { name, .. }| name.as_str()),
            &app.output_directory,
        )?;
    }

    Ok(())
}
