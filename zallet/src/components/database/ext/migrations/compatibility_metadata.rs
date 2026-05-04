use std::collections::HashSet;

use schemerz_rusqlite::RusqliteMigration;
use uuid::Uuid;
use zcash_client_sqlite::wallet::init::WalletMigrationError;

use super::initial_setup;

pub(super) const MIGRATION_ID: Uuid = Uuid::from_u128(0x0b371d41_93c0_4839_b087_b363fe94f028);

pub(super) struct Migration;

impl schemerz::Migration<Uuid> for Migration {
    fn id(&self) -> Uuid {
        MIGRATION_ID
    }

    fn dependencies(&self) -> HashSet<Uuid> {
        [initial_setup::MIGRATION_ID].into_iter().collect()
    }

    fn description(&self) -> &'static str {
        "Initializes the Zallet database compatibility metadata table."
    }
}

impl RusqliteMigration for Migration {
    type Error = WalletMigrationError;

    fn up(&self, transaction: &rusqlite::Transaction<'_>) -> Result<(), Self::Error> {
        transaction.execute_batch(
            "CREATE TABLE ext_zallet_db_compatibility_metadata (
                compatibility_epoch INTEGER NOT NULL,
                migrated TEXT NOT NULL
            );",
        )?;

        Ok(())
    }

    fn down(&self, _transaction: &rusqlite::Transaction<'_>) -> Result<(), Self::Error> {
        Ok(())
    }
}
