//! `trip store …`: search, look at, install and remove apps from Flathub.

use anyhow::{Result, anyhow, bail};
use std::io::{IsTerminal, Write};
use trip::store::{self, App, Catalog, Progress};

/// The catalogue, fetching it first if this computer has never had it.
fn catalog() -> Result<Catalog> {
    let mut c = Catalog::load();
    if c.apps.is_empty() {
        eprintln!("fetching the app catalogue from Flathub…");
        store::refresh().map_err(|e| anyhow!("couldn't fetch the catalogue: {e}"))?;
        c = Catalog::load();
        if c.apps.is_empty() {
            bail!("the catalogue is empty. Is the flathub remote set up? (flatpak remotes)");
        }
    }
    Ok(c)
}

fn pick<'a>(c: &'a Catalog, words: &[String]) -> Result<&'a App> {
    let q = words.join(" ");
    c.find(&q).ok_or_else(|| anyhow!("no app matches \"{q}\". Try: trip store search {q}"))
}

pub fn search(words: &[String]) -> Result<()> {
    let c = catalog()?;
    let have: Vec<String> = store::installed().into_iter().map(|i| i.id).collect();
    let hits = c.search(&words.join(" "), 15);
    if hits.is_empty() {
        println!("nothing found for \"{}\"", words.join(" "));
    }
    for a in hits {
        let mark = if have.contains(&a.id) { "installed" } else { "" };
        println!("{:<24} {:<10} {}", clip(&a.name, 24), mark, clip(&a.summary, 70));
        println!("  {}", a.id);
    }
    Ok(())
}

pub fn info(words: &[String]) -> Result<()> {
    let c = catalog()?;
    let a = pick(&c, words)?;
    println!("{}", a.name);
    println!("{}", a.summary);
    let mut facts = vec![];
    if !a.licence_words().is_empty() {
        facts.push(a.licence_words());
    }
    if !a.developer.is_empty() {
        facts.push(a.developer.clone());
    }
    if let Some(s) = a.size.map(store::size_words).or_else(|| store::download_size(&a.id)) {
        facts.push(s);
    }
    facts.push(if a.verified { "Flathub, verified".into() } else { "Flathub".into() });
    println!("{}\n  {}\n", facts.join(" · "), a.id);
    if !a.description.is_empty() {
        println!("{}\n", a.description);
    }
    println!("IT CAN USE");
    match store::permissions(&a.id) {
        Ok(p) if !p.is_empty() => {
            for line in p {
                println!("  {line}");
            }
            println!("  nothing else · it runs in its own sandbox");
        }
        Ok(_) => println!("  nothing · it runs in its own sandbox"),
        Err(e) => println!("  (couldn't read: {e})"),
    }
    Ok(())
}

fn bar(p: Progress) -> String {
    let n = ((p.fraction() * 20.0).round() as usize).min(20);
    format!("{}{} {:>3.0}%", "■".repeat(n), "·".repeat(20 - n), p.fraction() * 100.0)
}

fn with_bar(label: &str, run: impl FnOnce(&mut dyn FnMut(Progress)) -> Result<(), String>) -> Result<()> {
    let tty = std::io::stderr().is_terminal();
    let result = run(&mut |p| {
        if tty {
            eprint!("\r\x1b[2K{label}  {}", bar(p));
            let _ = std::io::stderr().flush();
        }
    });
    if tty {
        eprintln!();
    }
    result.map_err(|e| anyhow!(e))
}

pub fn install(words: &[String]) -> Result<()> {
    let c = catalog()?;
    let a = pick(&c, words)?;
    if store::installed().iter().any(|i| i.id == a.id) {
        println!("{} is already installed.", a.name);
        return Ok(());
    }
    with_bar(&format!("installing {}", a.name), |p| store::install(&a.id, p))?;
    println!("installed {} ({})", a.name, a.id);
    Ok(())
}

pub fn remove(words: &[String]) -> Result<()> {
    let q = words.join(" ");
    let app = store::installed()
        .into_iter()
        .find(|i| i.id == q || i.name.to_lowercase().contains(&q.to_lowercase()))
        .ok_or_else(|| anyhow!("\"{q}\" isn't installed. See: flatpak list --app"))?;
    with_bar(&format!("removing {}", app.name), |p| store::remove(&app.id, p))?;
    println!("removed {} ({})", app.name, app.id);
    Ok(())
}

pub fn updates(apply: bool) -> Result<()> {
    let _ = store::refresh();
    let ids = store::updates();
    if ids.is_empty() {
        println!("everything is up to date");
        return Ok(());
    }
    let have = store::installed();
    for id in &ids {
        let name = have.iter().find(|i| &i.id == id).map_or(id.as_str(), |i| i.name.as_str());
        println!("{name:<24} {id}");
    }
    let s = if ids.len() == 1 { "" } else { "s" };
    if apply {
        with_bar("updating", |p| store::update(None, p))?;
        println!("updated {} app{s}", ids.len());
    } else {
        println!("\n{} update{s} waiting. Run: trip store updates --apply", ids.len());
    }
    Ok(())
}

fn clip(s: &str, n: usize) -> String {
    if s.chars().count() <= n { s.to_string() } else { s.chars().take(n - 1).collect::<String>() + "…" }
}
