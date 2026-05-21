//! creatine-reminder — mail users who opted in but haven't logged creatine today.
//!
//! Usage:
//!   creatine-reminder            # print summary to stdout
//!   creatine-reminder --email    # print summary and send reminder emails
//!   creatine-reminder --dry-run  # print summary and show what would be sent
//!
//! Environment variables:
//!   DATABASE_URL      path to SQLite database (default: wasd5.db)
//!   SITE_URL          base URL used in email links (default: https://wasd.dk)
//!
//! SMTP — see wasd5::email for the full list of SMTP_* variables.

use std::env;

use chrono::Utc;
use clap::Parser;
use diesel::prelude::*;
use wasd5::db;
use wasd5::email::{SmtpConfig, send_email};
use wasd5::models::User;

// ---------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------

#[derive(Parser)]
#[command(
    name = "creatine-reminder",
    about = "Email users who haven't logged their creatine intake today"
)]
struct Args {
    /// Send reminder emails
    #[arg(long)]
    email: bool,

    /// Show what would be sent without actually delivering email
    #[arg(long)]
    dry_run: bool,
}

// ---------------------------------------------------------------------------
// Email body builders
// ---------------------------------------------------------------------------

fn build_plain(username: &str, site_url: &str) -> String {
    format!(
        "Hi {},\n\nThis is a friendly reminder to log your creatine intake for today.\n\nVisit {}/creatine to record it.\n\nStay consistent! 💪\n",
        username, site_url
    )
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn build_html(username: &str, site_url: &str) -> String {
    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width,initial-scale=1">
  <title>Creatine reminder</title>
</head>
<body style="font-family:system-ui,sans-serif;background:#f8fafc;margin:0;padding:24px">
  <div style="max-width:520px;margin:0 auto;background:#fff;border-radius:8px;border:1px solid #e2e8f0;padding:32px">

    <h2 style="margin-top:0;color:#0f172a;font-size:1.2rem">💪 Daily creatine reminder</h2>

    <p style="color:#374151;line-height:1.6">
      Hi <strong>{username}</strong>,<br><br>
      You haven't logged your creatine intake yet today.
      Don't break the streak!
    </p>

    <p style="margin-top:24px;margin-bottom:0">
      <a href="{url}/creatine"
         style="display:inline-block;background:#0d6efd;color:#fff;text-decoration:none;padding:10px 22px;border-radius:6px;font-weight:600;font-size:.9rem">
        Log intake now →
      </a>
    </p>

    <p style="margin-top:28px;padding-top:16px;border-top:1px solid #f1f5f9;color:#9ca3af;font-size:.78rem;line-height:1.5">
      You received this message because you enabled creatine reminders on
      <a href="{url}" style="color:#9ca3af">{url}</a>.
      You can turn them off on the <a href="{url}/creatine" style="color:#9ca3af">creatine tracker page</a>.
    </p>

  </div>
</body>
</html>"#,
        username = html_escape(username),
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

    let today = Utc::now().format("%Y-%m-%d").to_string();

    // ── Load users who have reminders enabled and have an email address ───────
    let reminder_users: Vec<User> = {
        use wasd5::schema::users::dsl::{creatine_reminder, email, users};
        users
            .filter(creatine_reminder.eq(1))
            .filter(email.is_not_null())
            .load::<User>(&mut conn)?
    };

    if reminder_users.is_empty() {
        println!("No users with creatine reminders enabled.");
        return Ok(());
    }

    // ── Filter to those who have NOT logged an intake today ───────────────────
    let mut needs_reminder: Vec<&User> = Vec::new();
    for user in &reminder_users {
        let taken_today: i64 = {
            use wasd5::schema::creatine_intakes::dsl::{creatine_intakes, date, user_id};
            creatine_intakes
                .filter(user_id.eq(user.id).and(date.eq(&today)))
                .count()
                .get_result(&mut conn)?
        };
        if taken_today == 0 {
            needs_reminder.push(user);
        }
    }

    // ── Print summary ─────────────────────────────────────────────────────────
    println!(
        "Today: {}  |  {}/{} opted-in users need a reminder\n",
        today,
        needs_reminder.len(),
        reminder_users.len()
    );

    for u in &needs_reminder {
        let email_label = u.email.as_deref().unwrap_or("(no email)");
        println!("  {} <{}>", u.username, email_label);
    }

    if needs_reminder.is_empty() {
        println!("Everyone has already logged creatine today. Nothing to send.");
        return Ok(());
    }

    if !args.email && !args.dry_run {
        println!("\nRun with --email to send reminders, or --dry-run to preview.");
        return Ok(());
    }

    // ── Email (or dry-run) ────────────────────────────────────────────────────
    let smtp = SmtpConfig::from_env();
    let subject = "💪 Don't forget your creatine today!";
    let mut sent = 0usize;

    for u in &needs_reminder {
        let to_addr = match u.email.as_deref() {
            Some(e) => e,
            None => continue, // already filtered, but be safe
        };

        let plain = build_plain(&u.username, &site_url);
        let html = build_html(&u.username, &site_url);

        if args.dry_run {
            println!("\n[dry-run] {} <{}>", u.username, to_addr);
            println!("  Subject : {}", subject);
            println!("  Body preview:");
            for line in plain.lines() {
                println!("    {}", line);
            }
        } else {
            match send_email(&smtp, to_addr, subject, &plain, &html) {
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

    if !args.dry_run {
        println!("\nDone — {} reminder(s) sent.", sent);
    }

    Ok(())
}
