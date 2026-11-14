//! The footer's wording about the account and its usage limits.

use mr_agent::{Account, LimitWindow, UsageLimits};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Normal,
    Warning,
    Alarm,
}

#[derive(Clone, Debug, PartialEq)]
pub struct UsageLine {
    pub text: String,
    /// How full the bar is, 0.0 ..= 1.0. `None` when the window is not known.
    pub fraction: Option<f32>,
    pub level: Level,
    pub resets: Option<String>,
}

/// The five-hour line, then the weekly line.
pub fn usage_lines(limits: &UsageLimits, now_unix: i64) -> [UsageLine; 2] {
    [
        line(limits.session.as_ref(), "current session", now_unix),
        line(limits.weekly.as_ref(), "weekly limit", now_unix),
    ]
}

fn line(window: Option<&LimitWindow>, name: &str, now_unix: i64) -> UsageLine {
    let Some(window) = window else {
        return UsageLine {
            text: "Usage not known yet".into(),
            fraction: None,
            level: Level::Normal,
            resets: None,
        };
    };
    let percent = window.percent();
    UsageLine {
        text: format!("{percent}% of {name} used"),
        fraction: Some(window.used.clamp(0.0, 1.0)),
        level: match percent {
            90.. => Level::Alarm,
            75.. => Level::Warning,
            _ => Level::Normal,
        },
        resets: Some(resets_in(window.resets_at_unix, now_unix)),
    }
}

pub fn resets_in(resets_at_unix: i64, now_unix: i64) -> String {
    let seconds = resets_at_unix - now_unix;
    let minutes = seconds / 60;
    let hours = minutes / 60;
    let days = hours / 24;
    if seconds <= 0 {
        "Resets soon".into()
    } else if minutes == 0 {
        "Resets in under a minute".into()
    } else if hours == 0 {
        format!("Resets in {minutes} min")
    } else if days == 0 {
        format!("Resets in {hours} h {} min", minutes % 60)
    } else {
        format!("Resets in {days} d {} h", hours % 24)
    }
}

/// Name and plan for the footer, such as "alex · Max".
pub fn account_line(account: &Account) -> String {
    match account {
        Account::SignedIn {
            name,
            plan: Some(plan),
            ..
        } => format!("{name} · {plan}"),
        Account::SignedIn { name, .. } => name.clone(),
        Account::SignedOut => "Not signed in".into(),
        Account::Missing => "Claude Code not found".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_lines_read_as_the_design_says() {
        let limits = UsageLimits {
            session: Some(LimitWindow {
                used: 0.37,
                resets_at_unix: 10_000 + 2 * 3600 + 14 * 60,
            }),
            weekly: None,
        };
        let [session, weekly] = usage_lines(&limits, 10_000);
        assert_eq!(
            session,
            UsageLine {
                text: "37% of current session used".into(),
                fraction: Some(0.37),
                level: Level::Normal,
                resets: Some("Resets in 2 h 14 min".into()),
            }
        );
        assert_eq!(
            weekly,
            UsageLine {
                text: "Usage not known yet".into(),
                fraction: None,
                level: Level::Normal,
                resets: None
            }
        );
    }

    #[test]
    fn the_level_rises_at_75_and_90() {
        let level = |used: f32| {
            usage_lines(
                &UsageLimits {
                    session: None,
                    weekly: Some(LimitWindow {
                        used,
                        resets_at_unix: 0,
                    }),
                },
                0,
            )[1]
            .level
        };
        assert_eq!(
            (
                level(0.74),
                level(0.75),
                level(0.89),
                level(0.90),
                level(1.0)
            ),
            (
                Level::Normal,
                Level::Warning,
                Level::Warning,
                Level::Alarm,
                Level::Alarm
            )
        );
        let [_, weekly] = usage_lines(
            &UsageLimits {
                session: None,
                weekly: Some(LimitWindow {
                    used: 1.0,
                    resets_at_unix: 0,
                }),
            },
            0,
        );
        assert_eq!(weekly.text, "100% of weekly limit used");
    }

    #[test]
    fn reset_times_are_said_in_words() {
        assert_eq!(resets_in(1_030, 1_000), "Resets in under a minute");
        assert_eq!(resets_in(1_000 + 14 * 60, 1_000), "Resets in 14 min");
        assert_eq!(
            resets_in(1_000 + 3 * 86_400 + 4 * 3600 + 59, 1_000),
            "Resets in 3 d 4 h"
        );
        assert_eq!(resets_in(999, 1_000), "Resets soon");
    }

    #[test]
    fn the_account_line_names_the_person_and_plan() {
        let signed = |plan: Option<&str>| Account::SignedIn {
            name: "alex".into(),
            plan: plan.map(str::to_string),
            full_login: true,
        };
        assert_eq!(account_line(&signed(Some("Max"))), "alex · Max");
        assert_eq!(account_line(&signed(None)), "alex");
        assert_eq!(account_line(&Account::SignedOut), "Not signed in");
        assert_eq!(account_line(&Account::Missing), "Claude Code not found");
    }
}
