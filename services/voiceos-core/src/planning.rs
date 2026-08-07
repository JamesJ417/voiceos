use chrono::{DateTime, Duration, Utc};

use crate::{ConversationStore, DailyWorkPlan, PlannedWorkBlock, StoreError};

impl ConversationStore {
    pub fn build_daily_work_plan(
        &self,
        owner_id: &str,
        day_start: &str,
        day_end: &str,
        current_location: &str,
    ) -> Result<DailyWorkPlan, StoreError> {
        let start = parse_time("day_start", day_start)?;
        let end = parse_time("day_end", day_end)?;
        if end <= start || end - start > Duration::hours(24) {
            return Err(StoreError::InvalidInput(
                "planning window must be between 1 minute and 24 hours".to_owned(),
            ));
        }
        let mut busy = self
            .calendar_events(owner_id, day_start, day_end)?
            .into_iter()
            .map(|event| {
                let event_start = parse_time("calendar start", &event.start_at)
                    .expect("validated calendar timestamp");
                let event_end = parse_time("calendar end", &event.end_at)
                    .expect("validated calendar timestamp");
                (
                    event_start
                        - Duration::minutes(i64::from(
                            event.travel_minutes + event.preparation_minutes,
                        )),
                    event_end,
                )
            })
            .collect::<Vec<_>>();
        busy.sort_by_key(|window| window.0);
        let mut tasks = self.tasks(owner_id, false, 200)?;
        tasks.retain(|task| task.status != "blocked");
        tasks.sort_by(|left, right| {
            let left_due = left.due_at.as_deref().unwrap_or("9999-12-31T23:59:59Z");
            let right_due = right.due_at.as_deref().unwrap_or("9999-12-31T23:59:59Z");
            left_due
                .cmp(right_due)
                .then_with(|| left.estimated_minutes.cmp(&right.estimated_minutes))
        });
        let mut cursor = start;
        let mut blocks = Vec::new();
        let mut unscheduled = Vec::new();
        let mut warnings = Vec::new();
        for task in tasks {
            let schedule = self.task_schedule(owner_id, &task.id)?;
            if let Some(earliest) = schedule
                .as_ref()
                .and_then(|value| value.earliest_start_at.as_deref())
            {
                cursor = cursor.max(parse_time("earliest_start_at", earliest)?);
            }
            let prep = schedule
                .as_ref()
                .map_or(0, |value| value.preparation_minutes);
            let travel = schedule.as_ref().map_or(0, |value| value.travel_minutes);
            let needed = Duration::minutes(i64::from(task.estimated_minutes + prep + travel));
            let mut candidate = cursor;
            loop {
                let conflict = busy.iter().find(|(busy_start, busy_end)| {
                    candidate < *busy_end && candidate + needed > *busy_start
                });
                match conflict {
                    Some((_, busy_end)) => candidate = *busy_end,
                    None => break,
                }
            }
            if candidate + needed > end {
                unscheduled.push(task.id.clone());
                continue;
            }
            let work_start = candidate + Duration::minutes(i64::from(prep + travel));
            let work_end = work_start + Duration::minutes(i64::from(task.estimated_minutes));
            let location = schedule.as_ref().and_then(|value| value.location.clone());
            let reason = match task.due_at.as_deref() {
                Some(due) => format!("Scheduled by earliest deadline; due {due}."),
                None => "Scheduled in the next available focus block.".to_owned(),
            };
            blocks.push(PlannedWorkBlock {
                task_id: task.id.clone(),
                title: task.title,
                start_at: work_start.to_rfc3339(),
                end_at: work_end.to_rfc3339(),
                location,
                preparation_minutes: prep,
                travel_minutes: travel,
                reason,
            });
            busy.push((candidate, work_end));
            busy.sort_by_key(|window| window.0);
            cursor = work_end;
        }
        if !unscheduled.is_empty() {
            warnings.push(format!(
                "{} task(s) do not fit in the available time and need replanning.",
                unscheduled.len()
            ));
        }
        Ok(DailyWorkPlan {
            owner_id: owner_id.to_owned(),
            date: start.date_naive().to_string(),
            generated_at: Utc::now().to_rfc3339(),
            current_location: current_location.to_owned(),
            blocks,
            unscheduled_task_ids: unscheduled,
            warnings,
        })
    }
}

fn parse_time(name: &str, value: &str) -> Result<DateTime<Utc>, StoreError> {
    DateTime::parse_from_rfc3339(value)
        .map(|value| value.with_timezone(&Utc))
        .map_err(|_| StoreError::InvalidInput(format!("{name} must be an RFC3339 timestamp")))
}
