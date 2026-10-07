use crate::error::{AppError, Result};
use crate::models::reminder::{
    CreateReminderRequest, RecurType, Reminder, ReminderFilter, ReminderState,
    UpdateReminderRequest,
};
use rusqlite::{params, Connection, OptionalExtension};
use uuid::Uuid;

pub fn create_reminder(
    conn: &Connection,
    user_id: &str,
    req: CreateReminderRequest,
) -> Result<Reminder> {
    let id = Uuid::new_v4().to_string();
    let recur_type_str = req.recur_type.map(|r| r.as_str());

    conn.execute(
        "INSERT INTO reminders (id, user_id, note_id, title, description, remind_at,
                                is_recurring, cron_expr, recur_type, state, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'pending', datetime('now'), datetime('now'))",
        params![
            id,
            user_id,
            req.note_id,
            req.title.trim(),
            req.description,
            req.remind_at,
            if req.is_recurring { 1 } else { 0 },
            req.cron_expr,
            recur_type_str,
        ],
    )?;

    get_reminder_by_id(conn, user_id, &id)?
        .ok_or_else(|| AppError::Internal("Reminder creation failed".into()))
}

pub fn get_reminder_by_id(
    conn: &Connection,
    user_id: &str,
    reminder_id: &str,
) -> Result<Option<Reminder>> {
    let mut stmt = conn.prepare(
        "SELECT id, user_id, note_id, title, description, remind_at,
                is_recurring, cron_expr, recur_type, state, snooze_until,
                created_at, updated_at, last_triggered
         FROM reminders
         WHERE id = ?1 AND user_id = ?2",
    )?;

    let reminder = stmt
        .query_row(params![reminder_id, user_id], |row| {
            let recur_type_str: Option<String> = row.get(8)?;
            let state_str: String = row.get(9)?;

            Ok(Reminder {
                id: row.get(0)?,
                user_id: row.get(1)?,
                note_id: row.get(2)?,
                title: row.get(3)?,
                description: row.get(4)?,
                remind_at: row.get(5)?,
                is_recurring: row.get::<_, i32>(6)? == 1,
                cron_expr: row.get(7)?,
                recur_type: recur_type_str.as_deref().and_then(RecurType::from_str_opt),
                state: ReminderState::from_str_opt(&state_str),
                snooze_until: row.get(10)?,
                created_at: row.get(11)?,
                updated_at: row.get(12)?,
                last_triggered: row.get(13)?,
            })
        })
        .optional()?;

    Ok(reminder)
}

pub fn list_reminders(
    conn: &Connection,
    user_id: &str,
    filter: &ReminderFilter,
) -> Result<(Vec<Reminder>, i64)> {
    let page = filter.page.unwrap_or(1).max(1);
    let limit = filter.limit.unwrap_or(20).clamp(1, 100);
    let offset = (page - 1) * limit;

    let state_str = filter.state.map(|s| s.as_str().to_string());
    let has_state = state_str.is_some();

    let has_note = filter.note_id.is_some();
    let note_id = filter.note_id.clone().unwrap_or_default();

    let upcoming_only = filter.upcoming_only.unwrap_or(false);

    // Total count
    let total: i64 = {
        let mut count_query = String::from("SELECT COUNT(*) FROM reminders WHERE user_id = ?1");
        if has_state {
            count_query.push_str(" AND state = ?2");
        }
        if has_note {
            count_query.push_str(" AND note_id = ?3");
        }
        if upcoming_only {
            count_query.push_str(" AND remind_at > datetime('now')");
        }

        let mut stmt = conn.prepare(&count_query)?;
        if has_state && has_note {
            stmt.query_row(params![user_id, state_str, note_id], |r| r.get(0))?
        } else if has_state {
            stmt.query_row(params![user_id, state_str], |r| r.get(0))?
        } else if has_note {
            stmt.query_row(params![user_id, note_id], |r| r.get(0))?
        } else {
            stmt.query_row(params![user_id], |r| r.get(0))?
        }
    };

    let mut data_query = String::from(
        "SELECT id, user_id, note_id, title, description, remind_at,
                is_recurring, cron_expr, recur_type, state, snooze_until,
                created_at, updated_at, last_triggered
         FROM reminders
         WHERE user_id = ?1",
    );

    if has_state {
        data_query.push_str(" AND state = ?2");
    }
    if has_note {
        data_query.push_str(" AND note_id = ?3");
    }
    if upcoming_only {
        data_query.push_str(" AND remind_at > datetime('now')");
    }

    data_query.push_str(" ORDER BY remind_at ASC LIMIT ?4 OFFSET ?5");

    let mut stmt = conn.prepare(&data_query)?;

    let map_row = |row: &rusqlite::Row| {
        let recur_type_str: Option<String> = row.get(8)?;
        let s_str: String = row.get(9)?;

        Ok(Reminder {
            id: row.get(0)?,
            user_id: row.get(1)?,
            note_id: row.get(2)?,
            title: row.get(3)?,
            description: row.get(4)?,
            remind_at: row.get(5)?,
            is_recurring: row.get::<_, i32>(6)? == 1,
            cron_expr: row.get(7)?,
            recur_type: recur_type_str.as_deref().and_then(RecurType::from_str_opt),
            state: ReminderState::from_str_opt(&s_str),
            snooze_until: row.get(10)?,
            created_at: row.get(11)?,
            updated_at: row.get(12)?,
            last_triggered: row.get(13)?,
        })
    };

    let rows: Vec<Reminder> = if has_state && has_note {
        stmt.query_map(params![user_id, state_str, note_id, limit, offset], map_row)?
            .filter_map(std::result::Result::ok)
            .collect()
    } else if has_state {
        stmt.query_map(params![user_id, state_str, limit, offset], map_row)?
            .filter_map(std::result::Result::ok)
            .collect()
    } else if has_note {
        stmt.query_map(params![user_id, note_id, limit, offset], map_row)?
            .filter_map(std::result::Result::ok)
            .collect()
    } else {
        stmt.query_map(params![user_id, limit, offset], map_row)?
            .filter_map(std::result::Result::ok)
            .collect()
    };

    Ok((rows, total))
}

pub fn update_reminder(
    conn: &Connection,
    user_id: &str,
    reminder_id: &str,
    req: UpdateReminderRequest,
) -> Result<Reminder> {
    let existing = get_reminder_by_id(conn, user_id, reminder_id)?
        .ok_or_else(|| AppError::NotFound(format!("Reminder {reminder_id} not found")))?;

    let title = req.title.unwrap_or(existing.title);
    let description = req.description.or(existing.description);
    let remind_at = req.remind_at.unwrap_or(existing.remind_at);
    let is_recurring = req.is_recurring.unwrap_or(existing.is_recurring);
    let cron_expr = req.cron_expr.or(existing.cron_expr);
    let recur_type = req.recur_type.or(existing.recur_type);
    let recur_type_str = recur_type.map(|r| r.as_str());
    let note_id = req.note_id.or(existing.note_id);

    conn.execute(
        "UPDATE reminders
         SET title = ?1, description = ?2, remind_at = ?3,
             is_recurring = ?4, cron_expr = ?5, recur_type = ?6,
             note_id = ?7, updated_at = datetime('now')
         WHERE id = ?8 AND user_id = ?9",
        params![
            title.trim(),
            description,
            remind_at,
            if is_recurring { 1 } else { 0 },
            cron_expr,
            recur_type_str,
            note_id,
            reminder_id,
            user_id
        ],
    )?;

    get_reminder_by_id(conn, user_id, reminder_id)?
        .ok_or_else(|| AppError::Internal("Reminder update failed".into()))
}

pub fn delete_reminder(conn: &Connection, user_id: &str, reminder_id: &str) -> Result<()> {
    let rows_affected = conn.execute(
        "DELETE FROM reminders WHERE id = ?1 AND user_id = ?2",
        params![reminder_id, user_id],
    )?;

    if rows_affected == 0 {
        return Err(AppError::NotFound(format!(
            "Reminder {reminder_id} not found"
        )));
    }

    Ok(())
}

pub fn snooze_reminder(
    conn: &Connection,
    user_id: &str,
    reminder_id: &str,
    duration_minutes: u32,
) -> Result<Reminder> {
    let _existing = get_reminder_by_id(conn, user_id, reminder_id)?
        .ok_or_else(|| AppError::NotFound(format!("Reminder {reminder_id} not found")))?;

    let modifier = format!("+{} minutes", duration_minutes.max(1));

    conn.execute(
        "UPDATE reminders
         SET state = 'snoozed',
             snooze_until = datetime('now', ?1),
             remind_at = datetime('now', ?1),
             updated_at = datetime('now')
         WHERE id = ?2 AND user_id = ?3",
        params![modifier, reminder_id, user_id],
    )?;

    get_reminder_by_id(conn, user_id, reminder_id)?
        .ok_or_else(|| AppError::Internal("Reminder snooze failed".into()))
}

pub fn acknowledge_reminder(
    conn: &Connection,
    user_id: &str,
    reminder_id: &str,
    action: &str,
) -> Result<Reminder> {
    let _existing = get_reminder_by_id(conn, user_id, reminder_id)?
        .ok_or_else(|| AppError::NotFound(format!("Reminder {reminder_id} not found")))?;

    let new_state = if action == "dismiss" {
        ReminderState::Dismissed
    } else {
        ReminderState::Acknowledged
    };

    conn.execute(
        "UPDATE reminders
         SET state = ?1, updated_at = datetime('now')
         WHERE id = ?2 AND user_id = ?3",
        params![new_state.as_str(), reminder_id, user_id],
    )?;

    get_reminder_by_id(conn, user_id, reminder_id)?
        .ok_or_else(|| AppError::Internal("Reminder acknowledge failed".into()))
}

pub fn get_due_reminders(conn: &Connection) -> Result<Vec<Reminder>> {
    let mut stmt = conn.prepare(
        "SELECT id, user_id, note_id, title, description, remind_at,
                is_recurring, cron_expr, recur_type, state, snooze_until,
                created_at, updated_at, last_triggered
         FROM reminders
         WHERE (state = 'pending' OR state = 'snoozed')
           AND remind_at <= datetime('now')",
    )?;

    let reminders = stmt
        .query_map([], |row| {
            let recur_type_str: Option<String> = row.get(8)?;
            let s_str: String = row.get(9)?;

            Ok(Reminder {
                id: row.get(0)?,
                user_id: row.get(1)?,
                note_id: row.get(2)?,
                title: row.get(3)?,
                description: row.get(4)?,
                remind_at: row.get(5)?,
                is_recurring: row.get::<_, i32>(6)? == 1,
                cron_expr: row.get(7)?,
                recur_type: recur_type_str.as_deref().and_then(RecurType::from_str_opt),
                state: ReminderState::from_str_opt(&s_str),
                snooze_until: row.get(10)?,
                created_at: row.get(11)?,
                updated_at: row.get(12)?,
                last_triggered: row.get(13)?,
            })
        })?
        .filter_map(std::result::Result::ok)
        .collect();

    Ok(reminders)
}

pub fn mark_reminder_triggered(conn: &Connection, reminder_id: &str) -> Result<()> {
    conn.execute(
        "UPDATE reminders
         SET state = 'triggered',
             last_triggered = datetime('now'),
             updated_at = datetime('now')
         WHERE id = ?1",
        params![reminder_id],
    )?;
    Ok(())
}

pub fn reschedule_recurring_reminder(
    conn: &Connection,
    reminder_id: &str,
    next_remind_at: &str,
) -> Result<()> {
    conn.execute(
        "UPDATE reminders
         SET state = 'pending',
             remind_at = ?1,
             snooze_until = NULL,
             last_triggered = datetime('now'),
             updated_at = datetime('now')
         WHERE id = ?2",
        params![next_remind_at, reminder_id],
    )?;
    Ok(())
}
