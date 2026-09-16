//! `--print`: plain-text summaries for scripts, CI logs and AI agents.

use crate::TabArg;
use crate::cache::Cache;
use crate::config::{Config, tilde};
use crate::sf::deploy::DeployRecord;
use crate::sf::orgs::OrgInfo;
use crate::sf::push::PushData;
use crate::sf::versions::PackageVersion;
use crate::sf::{deploy, orgs, push, versions};
use crate::{demo, ui};
use anyhow::{Context, Result};

pub fn run(config: &Config, tab: TabArg, org: Option<&str>, demo: bool) -> Result<()> {
    let hub = &config.dev_hub;
    if config.needs_setup() && matches!(tab, TabArg::Push | TabArg::Subscribers | TabArg::Versions) {
        anyhow::bail!(
            "no Dev Hub configured. Start sf-cockpit without --print and choose it on the Settings tab, \
             or pass --dev-hub <alias>. No Dev Hub logged in yet? Run: sf org login web --alias DevHub \
             --set-default-dev-hub"
        );
    }
    match tab {
        TabArg::Push | TabArg::Subscribers => {
            let data = if demo {
                demo::push()
            } else {
                push::load(hub, config.limit, config.package.as_deref())?
            };
            if tab == TabArg::Push {
                print_push(hub, &data);
            } else {
                print_subscribers(&data);
            }
        }
        TabArg::Orgs => {
            let list = if demo { demo::orgs() } else { orgs::load_orgs()? };
            print_orgs(&list);
        }
        TabArg::Versions => {
            let (data, list) = if demo {
                (demo::push(), demo::versions())
            } else {
                let data = push::load(hub, config.limit, config.package.as_deref())?;
                let package = data
                    .package
                    .as_ref()
                    .context("no package chosen, pick one on the Settings tab of sf-cockpit")?;
                let list = versions::load(hub, &package.id)?;
                (data, list)
            };
            print_versions(&data, &list);
        }
        TabArg::Deploys => {
            let org = org
                .map(str::to_string)
                .or_else(|| config.scratch_org.clone())
                .context("no org given, pass --org <alias> or set scratch_org in sf-cockpit.toml")?;
            let list = if demo {
                demo::deploys()
            } else {
                deploy::load_deploys(&org)?
            };
            print_deploys(&org, &list);
        }
        TabArg::Settings => print_settings(config, demo),
    }
    Ok(())
}

fn print_push(org: &str, data: &PushData) {
    println!(
        "{} push requests, {} subscribers on {org}\n",
        data.requests.len(),
        data.subscribers.len()
    );
    for request in &data.requests {
        let counts = data.counts(&request.id);
        println!(
            "{}  {:<9} {:<10} {} succeeded, {} failed, {} other  ({})",
            request.id,
            data.version_label(&request.version_id),
            request.status,
            counts.succeeded,
            counts.failed,
            counts.other,
            ui::fmt_duration(request.start, request.end),
        );
        for job in data
            .jobs_for(&request.id)
            .into_iter()
            .filter(|j| j.status == "Failed")
        {
            let alias = data.alias(&job.org_key).map(|o| o.alias.as_str()).unwrap_or("-");
            println!(
                "    ✗ {} [{}] {}",
                data.org_name(&job.org_key),
                job.org_key,
                alias
            );
            for error in data.errors_for(&job.id) {
                println!("      {} ({}): {}", error.title, error.kind, error.message);
            }
        }
    }
}

fn print_subscribers(data: &PushData) {
    let latest = data
        .latest_released
        .as_deref()
        .map(|id| data.version_label(id))
        .unwrap_or_else(|| "?".into());
    println!(
        "{} subscribers, latest released {latest}\n",
        data.subscribers.len()
    );
    let mut subscribers: Vec<_> = data.subscribers.iter().collect();
    subscribers.sort_by_key(|s| (data.is_latest(&s.version_id), s.name.to_lowercase()));
    for s in subscribers {
        let state = if data.is_latest(&s.version_id) {
            "up to date"
        } else {
            "behind"
        };
        let alias = data.alias(&s.org_key).map(|o| o.alias.as_str()).unwrap_or("-");
        println!(
            "{:<10} {:<9} {:<32} {:<12} {:<8} {}  {}",
            state,
            data.version_label(&s.version_id),
            s.name,
            s.org_type,
            s.instance,
            s.org_key,
            alias
        );
    }
}

fn print_orgs(orgs: &[OrgInfo]) {
    println!("{} orgs\n", orgs.len());
    for org in orgs {
        let default = if org.is_default {
            " (default)"
        } else if org.is_default_hub {
            " (default dev hub)"
        } else {
            ""
        };
        let expires = org.expires.map(|d| format!("  expires {d}")).unwrap_or_default();
        println!(
            "{:<10} {:<24} {:<40} {}  {}{}{}",
            org.kind.label(),
            org.aliases.join(", "),
            org.username,
            org.org_id,
            org.status,
            expires,
            default
        );
    }
}

fn print_versions(data: &PushData, versions: &[PackageVersion]) {
    println!("{} package versions\n", versions.len());
    for v in versions {
        let coverage = v
            .coverage
            .map(|c| format!("{c:.0}%"))
            .unwrap_or_else(|| "-".into());
        println!(
            "{:<10} {:<9} {:<20} {:<18} coverage {:<5} {:>3} subscribers  {}",
            v.state(),
            v.version,
            v.name,
            v.created,
            coverage,
            data.subscribers_on(&v.id),
            v.id
        );
    }
}

fn print_deploys(org: &str, deploys: &[DeployRecord]) {
    println!("{} deployments on {org}\n", deploys.len());
    for d in deploys {
        println!(
            "{:<12} {:<12} components {}/{} ({} errors)  tests {}/{} ({} errors)  {}  {}  {}",
            ui::fmt_date(d.start.or(d.created)),
            d.status,
            d.components_deployed,
            d.components_total,
            d.component_errors,
            d.tests_completed,
            d.tests_total,
            d.test_errors,
            if d.check_only { "validate" } else { "deploy" },
            d.created_by,
            d.error_message
        );
    }
}

fn print_settings(config: &Config, demo: bool) {
    let none = || "-".to_string();
    let rows = [
        ("dev_hub", config.dev_hub.clone()),
        ("package", config.package.clone().unwrap_or_else(none)),
        ("scratch_org", config.scratch_org.clone().unwrap_or_else(none)),
        ("limit", config.limit.to_string()),
        (
            "project_dir",
            config.project_dir.as_deref().map(tilde).unwrap_or_else(none),
        ),
    ];
    for (key, value) in rows {
        println!("{key:<12} {value:<45} {}", config.origin(key).label());
    }
    println!();
    println!(
        "saves to     {}",
        config.save_path.as_deref().map(tilde).unwrap_or_else(none)
    );
    match Cache::default_location().filter(|_| !demo) {
        Some(cache) => {
            let (count, bytes) = cache.summary();
            println!(
                "cache        {} ({count} entries, {} KB)",
                tilde(cache.dir()),
                bytes.div_ceil(1024)
            );
        }
        None => println!("cache        off"),
    }
}
