use rand::rngs::OsRng;
use rusqlite::{Connection, OptionalExtension, named_params};
use tempfile::tempdir;
use zcash_client_sqlite::{WalletDb, util::SystemClock, wallet::init::WalletMigrator};
use zcash_protocol::consensus::{self, NetworkType, Parameters};

use crate::{components::database, config::ZalletConfig, network::Network};

#[cfg(zallet_build = "wallet")]
use crate::components::keystore;

#[test]
fn verify_schema() {
    let mut conn = Connection::open_in_memory().unwrap();
    let mut db_data = WalletDb::from_connection(
        &mut conn,
        Network::Consensus(consensus::Network::MainNetwork),
        SystemClock,
        OsRng,
    );

    WalletMigrator::new()
        .with_external_migrations(database::all_external_migrations(
            db_data.params().network_type(),
        ))
        .init_or_migrate(&mut db_data)
        .unwrap();

    use regex::Regex;
    let re = Regex::new(r"\s+").unwrap();

    let verify_consistency = |query: &str, expected: &[&str]| {
        let mut stmt = conn.prepare(query).unwrap();
        let mut rows = stmt.query([]).unwrap();
        let mut expected_idx = 0;
        while let Some(row) = rows.next().unwrap() {
            let sql: String = row.get(0).unwrap();
            assert_eq!(
                re.replace_all(&sql, " "),
                re.replace_all(expected[expected_idx], " ").trim(),
            );
            expected_idx += 1;
        }
        assert_eq!(expected_idx, expected.len());
    };

    verify_consistency(
        "SELECT sql
        FROM sqlite_schema
        WHERE type = 'table' AND tbl_name LIKE 'ext_zallet_%'
        ORDER BY tbl_name",
        &[
            database::ext::TABLE_COMPATIBILITY_METADATA,
            database::ext::TABLE_VERSION_METADATA,
            database::ext::TABLE_WALLET_METADATA,
            #[cfg(zallet_build = "wallet")]
            keystore::db::TABLE_AGE_RECIPIENTS,
            #[cfg(zallet_build = "wallet")]
            keystore::db::TABLE_LEGACY_SEEDS,
            #[cfg(zallet_build = "wallet")]
            keystore::db::TABLE_MNEMONICS,
            #[cfg(zallet_build = "wallet")]
            keystore::db::TABLE_STANDALONE_SAPLING_KEYS,
            #[cfg(zallet_build = "wallet")]
            keystore::db::TABLE_STANDALONE_TRANSPARENT_KEYS,
        ],
    );

    verify_consistency(
        "SELECT sql
        FROM sqlite_master
        WHERE type = 'index' AND sql != '' AND name LIKE 'ext_zallet_%'
        ORDER BY tbl_name, name",
        &[],
    );

    verify_consistency(
        "SELECT sql
        FROM sqlite_schema
        WHERE type = 'view' AND tbl_name LIKE 'ext_zallet_%'
        ORDER BY tbl_name",
        &[],
    );
}

#[test]
fn legacy_alpha_3_database_is_rejected_before_migrations() {
    let datadir = tempdir().unwrap();
    let config = test_config(datadir.path(), NetworkType::Test);
    create_wallet_db(config.wallet_db_path(), Some("0.1.0-alpha.3"), None);

    let err = open_database(&config).expect_err("legacy alpha.3 database must be rejected");
    assert!(
        err.to_string().contains("fresh Zallet wallet"),
        "unexpected error: {err}",
    );

    let conn = Connection::open(config.wallet_db_path()).unwrap();
    assert!(
        !table_exists(&conn, "ext_zallet_db_compatibility_metadata"),
        "compatibility migration should not run before rejecting the database",
    );
    assert_eq!(
        latest_recorded_version(&conn),
        Some("0.1.0-alpha.3".to_string())
    );
}

#[test]
fn legacy_alpha_4_database_is_allowed_and_marked_current() {
    let datadir = tempdir().unwrap();
    let config = test_config(datadir.path(), NetworkType::Test);
    create_wallet_db(config.wallet_db_path(), Some("0.1.0-alpha.4"), None);

    open_database(&config).unwrap();

    let conn = Connection::open(config.wallet_db_path()).unwrap();
    assert_eq!(
        latest_recorded_compatibility_epoch(&conn),
        Some(database::CURRENT_ALPHA_DB_COMPATIBILITY_EPOCH),
    );
}

#[test]
fn current_compatibility_epoch_is_allowed_even_with_alpha_3_version() {
    let datadir = tempdir().unwrap();
    let config = test_config(datadir.path(), NetworkType::Test);
    create_wallet_db(
        config.wallet_db_path(),
        Some("0.1.0-alpha.3"),
        Some(database::CURRENT_ALPHA_DB_COMPATIBILITY_EPOCH),
    );

    open_database(&config).unwrap();
}

#[test]
fn future_compatibility_epoch_is_rejected() {
    let datadir = tempdir().unwrap();
    let config = test_config(datadir.path(), NetworkType::Test);
    create_wallet_db(
        config.wallet_db_path(),
        Some("0.1.0-alpha.4"),
        Some(database::CURRENT_ALPHA_DB_COMPATIBILITY_EPOCH + 1),
    );

    let err = open_database(&config).expect_err("future compatibility epoch must be rejected");
    assert!(
        err.to_string().contains("fresh Zallet wallet"),
        "unexpected error: {err}",
    );
}

#[test]
fn malformed_legacy_version_is_rejected() {
    let datadir = tempdir().unwrap();
    let config = test_config(datadir.path(), NetworkType::Test);
    create_wallet_db(config.wallet_db_path(), Some("not-a-version"), None);

    let err = open_database(&config).expect_err("malformed version must be rejected");
    assert!(
        err.to_string().contains("fresh Zallet wallet"),
        "unexpected error: {err}",
    );
}

#[test]
fn missing_legacy_version_is_rejected() {
    let datadir = tempdir().unwrap();
    let config = test_config(datadir.path(), NetworkType::Test);
    create_wallet_db(config.wallet_db_path(), None, None);

    let err = open_database(&config).expect_err("missing version must be rejected");
    assert!(
        err.to_string().contains("fresh Zallet wallet"),
        "unexpected error: {err}",
    );
}

#[test]
fn network_mismatch_still_reports_network_error() {
    let datadir = tempdir().unwrap();
    let config = test_config(datadir.path(), NetworkType::Test);
    create_wallet_db_for_network(
        config.wallet_db_path(),
        Network::Consensus(consensus::Network::MainNetwork),
        Some("0.1.0-alpha.4"),
        None,
    );

    let err = open_database(&config).expect_err("network mismatch must be rejected");
    assert!(
        err.to_string()
            .contains("The wallet database was created for network type"),
        "unexpected error: {err}",
    );
}

fn test_config(datadir: &std::path::Path, network_type: NetworkType) -> ZalletConfig {
    ZalletConfig {
        datadir: Some(datadir.to_path_buf()),
        consensus: crate::config::ConsensusSection {
            network: network_type,
            ..Default::default()
        },
        ..Default::default()
    }
}

fn open_database(config: &ZalletConfig) -> Result<(), crate::error::Error> {
    crate::i18n::load_languages(&[]);

    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let database = database::Database::open(config).await?;
            drop(database);
            Ok(())
        })
}

fn create_wallet_db(
    path: impl AsRef<std::path::Path>,
    version: Option<&str>,
    compatibility_epoch: Option<i64>,
) {
    create_wallet_db_for_network(
        path,
        Network::Consensus(consensus::Network::TestNetwork),
        version,
        compatibility_epoch,
    );
}

fn create_wallet_db_for_network(
    path: impl AsRef<std::path::Path>,
    network: Network,
    version: Option<&str>,
    compatibility_epoch: Option<i64>,
) {
    let mut conn = Connection::open(path).unwrap();
    let mut db_data = WalletDb::from_connection(&mut conn, network, SystemClock, OsRng);

    WalletMigrator::new()
        .with_external_migrations(database::all_external_migrations(
            db_data.params().network_type(),
        ))
        .init_or_migrate(&mut db_data)
        .unwrap();

    match compatibility_epoch {
        Some(compatibility_epoch) => {
            if !table_exists(&conn, "ext_zallet_db_compatibility_metadata") {
                conn.execute_batch(&format!("{};", database::ext::TABLE_COMPATIBILITY_METADATA))
                    .unwrap();
            }
            conn.execute(
                "INSERT INTO ext_zallet_db_compatibility_metadata
                 VALUES (:compatibility_epoch, :migrated)",
                named_params! {
                    ":compatibility_epoch": compatibility_epoch,
                    ":migrated": "2026-01-01 00:00:00Z",
                },
            )
            .unwrap();
        }
        None => {
            conn.execute(
                "DROP TABLE IF EXISTS ext_zallet_db_compatibility_metadata",
                [],
            )
            .unwrap();
            conn.execute(
                "DELETE FROM schemer_migrations
                 WHERE id = X'0B371D4193C04839B087B363FE94F028'",
                [],
            )
            .unwrap();
        }
    }

    if let Some(version) = version {
        conn.execute(
            "INSERT INTO ext_zallet_db_version_metadata
             VALUES (:version, NULL, NULL, :migrated)",
            named_params! {
                ":version": version,
                ":migrated": "2026-01-01 00:00:00Z",
            },
        )
        .unwrap();
    }
}

fn table_exists(conn: &Connection, table_name: &str) -> bool {
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
    .unwrap()
    .is_some()
}

fn latest_recorded_version(conn: &Connection) -> Option<String> {
    conn.query_row(
        "SELECT version
         FROM ext_zallet_db_version_metadata
         ORDER BY rowid DESC
         LIMIT 1",
        [],
        |row| row.get("version"),
    )
    .optional()
    .unwrap()
}

fn latest_recorded_compatibility_epoch(conn: &Connection) -> Option<i64> {
    conn.query_row(
        "SELECT compatibility_epoch
         FROM ext_zallet_db_compatibility_metadata
         ORDER BY rowid DESC
         LIMIT 1",
        [],
        |row| row.get("compatibility_epoch"),
    )
    .optional()
    .unwrap()
}
