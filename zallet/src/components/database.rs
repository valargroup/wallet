use std::fmt;

use abscissa_core::tracing::info;
use rusqlite::{OptionalExtension, named_params};
use schemerz_rusqlite::RusqliteMigration;
use semver::Version;
use tokio::fs;

use zcash_client_sqlite::wallet::init::{WalletMigrationError, WalletMigrator};
use zcash_protocol::consensus::{NetworkType, Parameters};

use crate::{
    config::ZalletConfig,
    error::{Error, ErrorKind},
    fl,
};

#[cfg(zallet_build = "wallet")]
use super::keystore;

mod connection;
pub(crate) use connection::DbConnection;

mod ext;

#[cfg(test)]
mod tests;

pub(crate) type DbHandle = deadpool::managed::Object<connection::WalletManager>;

// Increment this whenever a Zallet alpha introduces a wallet database change
// that older alpha releases must not open.
const CURRENT_ALPHA_DB_COMPATIBILITY_EPOCH: i64 = 1;
// Old databases without explicit epoch metadata are accepted only if the last
// recorded Zallet version is at least the release that introduced the current
// compatibility epoch.
const MIN_LEGACY_COMPATIBLE_ZALLET_VERSION: &str = "0.1.0-alpha.4";

/// Returns the full list of migrations defined in Zallet, to be applied alongside the
/// migrations internal to `zcash_client_sqlite`.
fn all_external_migrations(
    network_type: NetworkType,
) -> Vec<Box<dyn RusqliteMigration<Error = WalletMigrationError>>> {
    let migrations = ext::migrations::all(network_type);

    #[cfg(zallet_build = "wallet")]
    let migrations = migrations.chain(keystore::db::migrations::all());

    migrations.collect()
}

#[derive(Clone)]
pub(crate) struct Database {
    db_data_pool: connection::WalletPool,
}

impl fmt::Debug for Database {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Database").finish_non_exhaustive()
    }
}

impl Database {
    pub(crate) async fn open(config: &ZalletConfig) -> Result<Self, Error> {
        let path = config.wallet_db_path();

        let db_exists = fs::try_exists(&path)
            .await
            .map_err(|e| ErrorKind::Init.context(e))?;

        let db_data_pool = connection::pool(&path, config.consensus.network())?;

        let database = Self { db_data_pool };

        let handle = database.handle().await?;

        if db_exists {
            // Verify that the database matches the configured network type and a
            // compatible alpha epoch before we make any changes (including migrations,
            // some of which make use of the network params), to avoid leaving the
            // database in an inconsistent state. We can assume the network metadata
            // table is present, as it's added by the initial migrations.
            handle.with_raw(|conn, _| verify_existing_database(conn, config))?;

            info!("Applying latest database migrations");
        } else {
            info!("Creating empty database");
        }

        // Initialize the database before we go any further.
        handle.with_mut(|mut db_data| {
            match WalletMigrator::new()
                .with_external_migrations(all_external_migrations(db_data.params().network_type()))
                .init_or_migrate(&mut db_data)
            {
                Ok(()) => Ok(()),
                // TODO: KeyStore depends on Database, but we haven't finished
                // initializing both yet. We might need to write logic to either
                // defer initialization until later, or expose enough of the
                // keystore read logic to let us parse the keystore database here
                // before the KeyStore component is initialized.
                //       https://github.com/zcash/wallet/issues/18
                // TODO: Support multi-seed or seed-absent migrations.
                //       https://github.com/zcash/librustzcash/issues/1284
                Err(schemerz::MigratorError::Migration {
                    error: WalletMigrationError::SeedRequired,
                    ..
                }) => Err(ErrorKind::Init.context("TODO: Support seed-required migrations")),
                Err(e) => Err(ErrorKind::Init.context(e)),
            }?;

            Ok::<(), Error>(())
        })?;

        let now = ::time::OffsetDateTime::now_utc();

        // Record that this database has reached the current compatibility epoch and was
        // opened using this Zallet version. We don't have an easy way to detect whether
        // any migrations actually ran, so we check whether the most recent version
        // entry matches the current version tuple, and only record an entry if it
        // doesn't.
        handle.with_raw_mut(|conn, _| {
            record_current_compatibility_epoch(conn, now)?;

            #[allow(clippy::const_is_empty)]
            let (git_revision, clean) = (!crate::build::COMMIT_HASH.is_empty())
                .then_some((crate::build::COMMIT_HASH, crate::build::GIT_CLEAN))
                .unzip();

            match conn
                .query_row(
                    "SELECT version, git_revision, clean
                    FROM ext_zallet_db_version_metadata
                    ORDER BY rowid DESC
                    LIMIT 1",
                    [],
                    |row| {
                        Ok(
                            row.get::<_, String>("version")? == crate::build::PKG_VERSION
                                && row.get::<_, Option<String>>("git_revision")?.as_deref()
                                    == git_revision
                                && row.get::<_, Option<bool>>("clean")? == clean,
                        )
                    },
                )
                .optional()
                .map_err(|e| ErrorKind::Init.context(e))?
            {
                Some(true) => (),
                None | Some(false) => {
                    conn.execute(
                        "INSERT INTO ext_zallet_db_version_metadata
                        VALUES (:version, :git_revision, :clean, :migrated)",
                        named_params! {
                            ":version": crate::build::PKG_VERSION,
                            ":git_revision": git_revision,
                            ":clean": clean,
                            ":migrated": now,
                        },
                    )
                    .map_err(|e| ErrorKind::Init.context(e))?;
                }
            }

            Ok::<(), Error>(())
        })?;

        Ok(database)
    }

    pub(crate) async fn handle(&self) -> Result<DbHandle, Error> {
        self.db_data_pool
            .get()
            .await
            .map_err(|e| ErrorKind::Generic.context(e).into())
    }
}

fn verify_existing_database(
    conn: &rusqlite::Connection,
    config: &ZalletConfig,
) -> Result<(), Error> {
    verify_wallet_network_type(conn, config)?;
    verify_alpha_db_compatibility(conn)
}

fn verify_wallet_network_type(
    conn: &rusqlite::Connection,
    config: &ZalletConfig,
) -> Result<(), Error> {
    let wallet_network_type = conn
        .query_row(
            "SELECT network_type FROM ext_zallet_db_wallet_metadata",
            [],
            |row| row.get::<_, crate::network::kind::Sql>("network_type"),
        )
        .map_err(|e| ErrorKind::Init.context(e))?;

    if wallet_network_type.0 == config.consensus.network {
        Ok(())
    } else {
        Err(ErrorKind::Init
            .context(fl!(
                "err-init-config-db-mismatch",
                db_network_type = crate::network::kind::type_to_str(&wallet_network_type.0),
                config_network_type = crate::network::kind::type_to_str(&config.consensus.network),
            ))
            .into())
    }
}

fn verify_alpha_db_compatibility(conn: &rusqlite::Connection) -> Result<(), Error> {
    if let Some(epoch) = latest_compatibility_epoch(conn)? {
        // A recorded epoch means the database has crossed an explicit compatibility
        // boundary. Only the exact epoch this binary understands is safe: lower
        // epochs are old incompatible alphas, and higher epochs were touched by a
        // newer alpha whose schema or semantics this binary may not understand.
        return if epoch == CURRENT_ALPHA_DB_COMPATIBILITY_EPOCH {
            Ok(())
        } else {
            Err(incompatible_alpha_database_error())
        };
    }

    let latest_version = latest_recorded_zallet_version(conn)?;
    let latest_version =
        Version::parse(&latest_version).map_err(|_| incompatible_alpha_database_error())?;
    let minimum_version = Version::parse(MIN_LEGACY_COMPATIBLE_ZALLET_VERSION)
        .expect("minimum compatible Zallet version is valid SemVer");

    if latest_version >= minimum_version {
        Ok(())
    } else {
        Err(incompatible_alpha_database_error())
    }
}

fn latest_compatibility_epoch(conn: &rusqlite::Connection) -> Result<Option<i64>, Error> {
    if !table_exists(conn, "ext_zallet_db_compatibility_metadata")
        .map_err(|_| incompatible_alpha_database_error())?
    {
        return Ok(None);
    }

    conn.query_row(
        "SELECT compatibility_epoch
         FROM ext_zallet_db_compatibility_metadata
         ORDER BY rowid DESC
         LIMIT 1",
        [],
        |row| row.get("compatibility_epoch"),
    )
    .optional()
    .map_err(|_| incompatible_alpha_database_error())
}

fn latest_recorded_zallet_version(conn: &rusqlite::Connection) -> Result<String, Error> {
    conn.query_row(
        "SELECT version
         FROM ext_zallet_db_version_metadata
         ORDER BY rowid DESC
         LIMIT 1",
        [],
        |row| row.get("version"),
    )
    .optional()
    .map_err(|_| incompatible_alpha_database_error())?
    .ok_or_else(incompatible_alpha_database_error)
}

fn table_exists(conn: &rusqlite::Connection, table_name: &str) -> rusqlite::Result<bool> {
    conn.query_row(
        "SELECT 1
         FROM sqlite_schema
         WHERE type = 'table' AND name = :table_name",
        named_params! {
            ":table_name": table_name,
        },
        |_| Ok(()),
    )
    .optional()
    .map(|row| row.is_some())
}

fn record_current_compatibility_epoch(
    conn: &rusqlite::Connection,
    now: ::time::OffsetDateTime,
) -> Result<(), Error> {
    let latest_epoch = conn
        .query_row(
            "SELECT compatibility_epoch
             FROM ext_zallet_db_compatibility_metadata
             ORDER BY rowid DESC
             LIMIT 1",
            [],
            |row| row.get::<_, i64>("compatibility_epoch"),
        )
        .optional()
        .map_err(|e| ErrorKind::Init.context(e))?;

    if latest_epoch.is_none_or(|epoch| epoch < CURRENT_ALPHA_DB_COMPATIBILITY_EPOCH) {
        conn.execute(
            "INSERT INTO ext_zallet_db_compatibility_metadata
             VALUES (:compatibility_epoch, :migrated)",
            named_params! {
                ":compatibility_epoch": CURRENT_ALPHA_DB_COMPATIBILITY_EPOCH,
                ":migrated": now,
            },
        )
        .map_err(|e| ErrorKind::Init.context(e))?;
    }

    Ok(())
}

fn incompatible_alpha_database_error() -> Error {
    ErrorKind::Init
        .context(fl!("err-init-db-incompatible-alpha"))
        .into()
}
