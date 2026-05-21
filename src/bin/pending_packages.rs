//! pending-packages — list users with pending packages and optionally email them.
//!
//! Usage:
//!   pending-packages              # print summary to stdout
//!   pending-packages --email      # print summary and send reminder emails
//!   pending-packages --dry-run    # print summary and show what would be emailed
//!
//! Environment variables:
//!   DATABASE_URL      path to the SQLite database (default: wasd5.db)
//!   SITE_URL          base URL used in email links (default: https://wasd.dk)
//!
//! SMTP — see wasd5::email for the full list of SMTP_* variables.

use std::collections::HashMap;
use std::env;

use chrono::{TimeZone, Utc};
use clap::Parser;
use diesel::prelude::*;
use wasd5::db;
use wasd5::email::{SmtpConfig, send_email};
use wasd5::models::{Package, User};

// ---------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------

#[derive(Parser)]
#[command(
    name = "pending-packages",
    about = "List users with pending packages; optionally send reminder emails"
)]
struct Args {
    /// Send reminder emails to users who have an email address registered
    #[arg(long)]
    email: bool,

    /// Show what would be sent without actually delivering any email
    #[arg(long)]
    dry_run: bool,
}

// ---------------------------------------------------------------------------
// Formatting helpers
// ---------------------------------------------------------------------------

fn fmt_date(ts: i64) -> String {
    Utc.timestamp_opt(ts, 0)
        .single()
        .map(|dt| dt.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| "?".to_string())
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

// ---------------------------------------------------------------------------
// Email body builders
// ---------------------------------------------------------------------------

fn build_plain(username: &str, pkgs: &[Package], site_url: &str) -> String {
    let mut out = format!(
        "Hi {},\n\nYou have {} pending package(s) waiting to be received:\n\n",
        username,
        pkgs.len()
    );
    for p in pkgs {
        let tracking = p.tracking_id.as_deref().unwrap_or("—");
        out.push_str(&format!("  • {}\n", p.name));
        out.push_str(&format!("    Ordered:  {}\n", fmt_date(p.ordered_date)));
        out.push_str(&format!("    Tracking: {}\n\n", tracking));
    }
    out.push_str(&format!(
        "Visit {}/packages to mark your packages as received.\n",
        site_url
    ));
    out
}

fn build_html(username: &str, pkgs: &[Package], site_url: &str) -> String {
    let rows: String = pkgs
        .iter()
        .map(|p| {
            let tracking = p.tracking_id.as_deref().unwrap_or("—");
            format!(
                "<tr>\
                   <td style=\"padding:8px 14px;border-bottom:1px solid #e5e7eb\">{name}</td>\
                   <td style=\"padding:8px 14px;border-bottom:1px solid #e5e7eb;white-space:nowrap\">{date}</td>\
                   <td style=\"padding:8px 14px;border-bottom:1px solid #e5e7eb;font-family:monospace;font-size:.85em\">{tracking}</td>\
                 </tr>",
                name = html_escape(&p.name),
                date = fmt_date(p.ordered_date),
                tracking = html_escape(tracking),
            )
        })
        .collect();

    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width,initial-scale=1">
  <title>Pending packages</title>
</head>
<body style="font-family:system-ui,sans-serif;background:#f8fafc;margin:0;padding:24px">
  <div style="max-width:580px;margin:0 auto;background:#fff;border-radius:8px;border:1px solid #e2e8f0;padding:32px">

    <h2 style="margin-top:0;color:#0f172a;font-size:1.2rem">📦 Pending packages</h2>

    <p style="color:#374151;line-height:1.6">
      Hi <strong>{username}</strong>,<br>
      you have <strong>{n} pending package(s)</strong> that have not yet been marked as received.
    </p>

    <table style="width:100%;border-collapse:collapse;font-size:.9rem;margin:16px 0">
      <thead>
        <tr style="background:#f1f5f9">
          <th style="padding:8px 14px;text-align:left;color:#6b7280;font-size:.72rem;text-transform:uppercase;letter-spacing:.05em">Description</th>
          <th style="padding:8px 14px;text-align:left;color:#6b7280;font-size:.72rem;text-transform:uppercase;letter-spacing:.05em">Ordered</th>
          <th style="padding:8px 14px;text-align:left;color:#6b7280;font-size:.72rem;text-transform:uppercase;letter-spacing:.05em">Tracking ID</th>
        </tr>
      </thead>
      <tbody>{rows}</tbody>
    </table>

    <p style="margin-top:24px;margin-bottom:0">
      <a href="{url}/packages"
         style="display:inline-block;background:#0d6efd;color:#fff;text-decoration:none;padding:10px 22px;border-radius:6px;font-weight:600;font-size:.9rem">
        View my packages →
      </a>
    </p>

    <p style="margin-top:28px;padding-top:16px;border-top:1px solid #f1f5f9;color:#9ca3af;font-size:.78rem;line-height:1.5">
      You received this message because your account on
      <a href="{url}" style="color:#9ca3af">{url}</a>
      has pending packages.
    </p>

  </div>
</body>
</html>"#,
        username = html_escape(username),
        n = pkgs.len(),
        rows = rows,
        url = site_url,
    )
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync + 'static>> {
    let args = Args::parse();
    let database_url = env::var("DATABASE_URL").unwrap_or_else(|_| "wasd5.db".to_string());
    let site_url = env::var("SITE_URL").unwrap_or_else(|_| "https://wasd.dk".to_string());

    let pool = db::establish_pool(&database_url);
    let mut conn = pool.get()?;

    // ── Load all pending packages, oldest first ──────────────────────────────
    let pending: Vec<Package> = {
        use wasd5::schema::packages::dsl::*;
        packages
            .filter(received_date.is_null())
            .order(ordered_date.asc())
            .load::<Package>(&mut conn)?
    };

    if pending.is_empty() {
        println!("No pending packages.");
        return Ok(());
    }

    // ── Group by user_id ─────────────────────────────────────────────────────
    let mut by_user: HashMap<i32, Vec<Package>> = HashMap::new();
    for pkg in pending {
        by_user.entry(pkg.user_id).or_default().push(pkg);
    }

    // ── Load the relevant users ───────────────────────────────────────────────
    let user_ids: Vec<i32> = by_user.keys().copied().collect();
    let mut user_list: Vec<User> = {
        use wasd5::schema::users::dsl::*;
        users.filter(id.eq_any(&user_ids)).load::<User>(&mut conn)?
    };
    user_list.sort_by(|a, b| a.username.cmp(&b.username));

    // ── Print summary ─────────────────────────────────────────────────────────
    let total: usize = by_user.values().map(|v| v.len()).sum();
    println!(
        "{} pending package(s) across {} user(s):\n",
        total,
        user_list.len()
    );

    for u in &user_list {
        let pkgs = match by_user.get(&u.id) {
            Some(p) => p,
            None => continue,
        };
        let email_label = u
            .email
            .as_deref()
            .map(|e| format!("<{}>", e))
            .unwrap_or_else(|| "(no email)".to_string());

        println!("  {} {}:", u.username, email_label);
        for p in pkgs {
            let tracking = p.tracking_id.as_deref().unwrap_or("—");
            println!(
                "    • {:<40} ordered {}   tracking {}",
                p.name,
                fmt_date(p.ordered_date),
                tracking
            );
        }
        println!();
    }

    // ── Email (or dry-run) ────────────────────────────────────────────────────
    if !args.email && !args.dry_run {
        return Ok(());
    }

    let smtp = SmtpConfig::from_env();
    let mut sent = 0usize;
    let mut skipped = 0usize;

    for u in &user_list {
        let pkgs = match by_user.get(&u.id) {
            Some(p) => p,
            None => continue,
        };

        let subject = format!("📦 {} pending package(s) waiting for you", pkgs.len());
        let plain = build_plain(&u.username, pkgs, &site_url);
        let html = build_html(&u.username, pkgs, &site_url);

        match &u.email {
            None => {
                println!("[skip]  {} — no email address registered", u.username);
                skipped += 1;
            }
            Some(to_addr) => {
                if args.dry_run {
                    println!("[dry-run]  {} <{}>", u.username, to_addr);
                    println!("  Subject : {}", subject);
                    println!("  Plain text body:");
                    for line in plain.lines() {
                        println!("    {}", line);
                    }
                    println!();
                } else {
                    match send_email(&smtp, to_addr, &subject, &plain, &html) {
                        Ok(()) => {
                            println!("[sent]  {} <{}>", u.username, to_addr);
                            sent += 1;
                        }
                        Err(e) => {
                            eprintln!("[error] {} <{}>: {}", u.username, to_addr, e);
                        }
                    }
                }
            }
        }
    }

    if !args.dry_run {
        println!("\nDone — {} sent, {} skipped (no email).", sent, skipped);
    }

    Ok(())
}
