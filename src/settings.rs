//! Persistent destination IDs for future runs; access tokens are never stored.
use crate::{Error, Result, execution::Destination, queue::QueueSummary, storage::Store};
use rusqlite::{OptionalExtension, params};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DestinationSettings {
    pub version: i64,
    pub personal_account: String,
    pub personal_pot: String,
    pub joint_account: String,
    pub joint_pot: String,
}

impl DestinationSettings {
    fn destinations(&self) -> Result<Vec<Destination>> {
        let mut destinations = Vec::new();
        for (allocation, account, pot) in [
            ("personal", &self.personal_account, &self.personal_pot),
            ("joint", &self.joint_account, &self.joint_pot),
        ] {
            if account.is_empty() && pot.is_empty() {
                continue;
            }
            let destination = Destination {
                allocation: allocation.into(),
                account_id: account.clone(),
                pot_id: pot.clone(),
            };
            destination.validate()?;
            destinations.push(destination);
        }
        if destinations.len() == 2
            && (self.personal_account == self.joint_account || self.personal_pot == self.joint_pot)
        {
            return Err(Error::Invalid(
                "Personal and Joint must use different accounts and pots.".into(),
            ));
        }
        Ok(destinations)
    }

    pub fn targets(&self, summary: &QueueSummary) -> Result<Vec<(Destination, i64)>> {
        let destinations = self.destinations()?;
        let mut targets = Vec::new();
        for (allocation, count, total) in [
            ("personal", summary.personal_count, summary.personal_pence),
            ("joint", summary.joint_count, summary.joint_pence),
        ] {
            if count == 0 {
                continue;
            }
            let destination = destinations.iter().find(|d| d.allocation == allocation)
                .ok_or_else(|| Error::Invalid(format!("Save the {allocation} account and pot IDs in Configuration before running recon.")))?;
            targets.push((destination.clone(), total));
        }
        Ok(targets)
    }
}

impl Store {
    pub fn destination_settings(&self) -> Result<DestinationSettings> {
        Ok(self.db.query_row(
            "SELECT version,personal_account,personal_pot,joint_account,joint_pot FROM destination_settings ORDER BY version DESC LIMIT 1", [],
            |r| Ok(DestinationSettings {version:r.get(0)?,personal_account:r.get(1)?,personal_pot:r.get(2)?,joint_account:r.get(3)?,joint_pot:r.get(4)?}),
        ).optional()?.unwrap_or_default())
    }

    pub fn save_destination_settings(&mut self, mut settings: DestinationSettings) -> Result<()> {
        for value in [
            &mut settings.personal_account,
            &mut settings.personal_pot,
            &mut settings.joint_account,
            &mut settings.joint_pot,
        ] {
            *value = value.trim().to_owned();
        }
        settings.destinations()?;
        self.backup()?;
        self.atomic(|s| {
            let current = s.destination_settings()?;
            if settings.version != current.version { return Err(Error::Stale); }
            if settings == current { return Ok(()); }
            s.db.execute(
                "INSERT INTO destination_settings(version,personal_account,personal_pot,joint_account,joint_pot) VALUES(?1,?2,?3,?4,?5)",
                params![current.version+1,settings.personal_account,settings.personal_pot,settings.joint_account,settings.joint_pot],
            )?;
            Ok(())
        })
    }
}
