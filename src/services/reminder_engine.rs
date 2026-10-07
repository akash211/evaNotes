use crate::db::{
    reminders::{get_due_reminders, mark_reminder_triggered, reschedule_recurring_reminder},
    DbPool,
};
use crate::services::notification::NotificationHub;
use chrono::Utc;
use cron::Schedule;
use std::str::FromStr;
use std::time::Duration;

pub fn compute_next_cron_trigger(cron_expr: &str) -> Option<String> {
    let trimmed = cron_expr.trim();
    let fields_count = trimmed.split_whitespace().count();

    // The Rust `cron` crate requires: `sec min hour day_of_month month day_of_week [year]`
    // If standard 5 fields provided, prepend "0 " and append " *"
    let formatted_expr = match fields_count {
        5 => format!("0 {} *", trimmed),
        6 => format!("{} *", trimmed),
        7 => trimmed.to_string(),
        _ => return None,
    };

    let schedule = Schedule::from_str(&formatted_expr).ok()?;
    let next = schedule.upcoming(Utc).next()?;
    Some(next.to_rfc3339())
}

pub async fn run_reminder_engine(pool: DbPool, hub: NotificationHub, poll_interval: Duration) {
    tracing::info!(
        "Reminder background engine started (polling every {:?})",
        poll_interval
    );
    let mut interval = tokio::time::interval(poll_interval);

    loop {
        interval.tick().await;

        let conn = match pool.get() {
            Ok(c) => c,
            Err(e) => {
                tracing::error!("Reminder engine failed to acquire DB connection: {e}");
                continue;
            }
        };

        let due_reminders = match get_due_reminders(&conn) {
            Ok(list) => list,
            Err(e) => {
                tracing::error!("Reminder engine query failed: {e}");
                continue;
            }
        };

        for reminder in due_reminders {
            tracing::info!(
                "Triggering reminder '{}' (ID: {})",
                reminder.title,
                reminder.id
            );

            // Broadcast notification via WebSocket
            hub.broadcast_reminder(&reminder);

            if reminder.is_recurring {
                if let Some(ref expr) = reminder.cron_expr {
                    if let Some(next_remind_at) = compute_next_cron_trigger(expr) {
                        if let Err(e) =
                            reschedule_recurring_reminder(&conn, &reminder.id, &next_remind_at)
                        {
                            tracing::error!("Failed to reschedule recurring reminder: {e}");
                        } else {
                            tracing::info!(
                                "Recurring reminder '{}' rescheduled to {}",
                                reminder.title,
                                next_remind_at
                            );
                        }
                        continue;
                    }
                }
            }

            // If not recurring or cron calculation failed, mark triggered
            if let Err(e) = mark_reminder_triggered(&conn, &reminder.id) {
                tracing::error!("Failed to mark reminder triggered: {e}");
            }
        }
    }
}
